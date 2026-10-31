//! Read-only acknowledgement for text containing UTF-16 surrogate pairs.
//! Queue insertion alone cannot authorize foreground handoff between their
//! WM_CHAR deliveries. Never repair or replay text when acknowledgement fails.
use super::input;
use crate::{backends::native_client::failed, computer::Error};
use std::time::{Duration, Instant};
use windows::{
    core::BSTR,
    Win32::{
        Foundation::HWND,
        UI::{Accessibility::*, WindowsAndMessaging::GetForegroundWindow},
    },
};

const MAX_UNITS: i32 = 65_536;
const SETTLE: Duration = Duration::from_millis(200);

pub(super) struct TextDelivery {
    uia: IUIAutomation,
    element: IUIAutomationElement,
    reader: Reader,
    window: HWND,
    prefix: String,
    suffix: String,
    typed: String,
    pending: bool,
    partial: bool,
}
enum Reader {
    Text(IUIAutomationTextPattern),
    Edit(super::edit_delivery::Edit),
}
impl Reader {
    fn text(&self) -> Result<String, Error> {
        match self {
            Self::Text(pattern) => unsafe { pattern.DocumentRange() }
                .map_err(failed)
                .and_then(|range| text(&range)),
            Self::Edit(edit) => edit.text(),
        }
    }
}

fn text(range: &IUIAutomationTextRange) -> Result<String, Error> {
    let raw = unsafe { range.GetText(MAX_UNITS + 1) }.map_err(failed)?;
    if raw.len() > MAX_UNITS as usize {
        return Err(failed(
            "Unicode delivery text exceeds the bounded read budget",
        ));
    }
    strict_text(&raw)
}
fn strict_text(raw: &BSTR) -> Result<String, Error> {
    String::from_utf16(raw.as_wide()).map_err(|_| failed("Native text contains incomplete UTF-16"))
}
fn normalize(text: &str) -> String {
    crate::backends::input_sequence::normalized_text(text).replace('\r', "\n")
}

impl TextDelivery {
    // The caller proves that element belongs to the exact retained window both
    // before and after these potentially blocking getters, before any typing.
    pub(super) fn new(
        uia: IUIAutomation,
        element: IUIAutomationElement,
        window: HWND,
    ) -> Result<Self, Error> {
        unsafe {
            if element.CurrentIsPassword().map_err(failed)?.as_bool() {
                return Err(Error::UnsupportedCapability);
            }
            let pattern = super::native::optional_pattern::<IUIAutomationTextPattern>(
                &element,
                UIA_TextPatternId,
            )?;
            let Some(pattern) = pattern else {
                let edit = super::edit_delivery::Edit::new(window)?;
                let (prefix, suffix) = edit.selection()?;
                return Ok(Self {
                    uia,
                    element,
                    reader: Reader::Edit(edit),
                    window,
                    prefix: normalize(&prefix),
                    suffix: normalize(&suffix),
                    typed: String::new(),
                    pending: false,
                    partial: false,
                });
            };
            let document = pattern.DocumentRange().map_err(failed)?;
            let full = text(&document)?;
            let ranges = pattern.GetSelection().map_err(failed)?;
            if ranges.Length().map_err(failed)? != 1 {
                return Err(Error::UnsupportedCapability);
            }
            let selection = ranges.GetElement(0).map_err(failed)?;
            if selection
                .CompareEndpoints(
                    TextPatternRangeEndpoint_Start,
                    &document,
                    TextPatternRangeEndpoint_Start,
                )
                .map_err(failed)?
                < 0
                || selection
                    .CompareEndpoints(
                        TextPatternRangeEndpoint_End,
                        &document,
                        TextPatternRangeEndpoint_End,
                    )
                    .map_err(failed)?
                    > 0
            {
                return Err(Error::InvalidObservation);
            }
            let before = document.Clone().map_err(failed)?;
            before
                .MoveEndpointByRange(
                    TextPatternRangeEndpoint_End,
                    &selection,
                    TextPatternRangeEndpoint_Start,
                )
                .map_err(failed)?;
            let after = document.Clone().map_err(failed)?;
            after
                .MoveEndpointByRange(
                    TextPatternRangeEndpoint_Start,
                    &selection,
                    TextPatternRangeEndpoint_End,
                )
                .map_err(failed)?;
            let prefix = text(&before)?;
            let selected = text(&selection)?;
            let suffix = text(&after)?;
            if format!("{prefix}{selected}{suffix}") != full || text(&document)? != full {
                return Err(failed(
                    "Focused native text changed before Unicode input admission",
                ));
            }
            Ok(Self {
                uia,
                element,
                reader: Reader::Text(pattern),
                window,
                prefix: normalize(&prefix),
                suffix: normalize(&suffix),
                typed: String::new(),
                pending: false,
                partial: false,
            })
        }
    }

    pub(super) fn inserted(&mut self, scalar: char, inserted: usize, total: usize) {
        if inserted == 0 {
            return;
        }
        self.pending = true;
        if inserted != total {
            self.partial = true;
        } else {
            self.typed.push(if scalar == '\r' { '\n' } else { scalar });
        }
    }

    pub(super) fn settle(&mut self, validate: impl Fn() -> Result<(), Error>) -> Result<(), Error> {
        if !self.pending {
            return Ok(());
        }
        if self.partial {
            return Err(failed(
                "Unicode input was only partially inserted; delivery is unconfirmed",
            ));
        }
        let expected = format!("{}{}{}", self.prefix, self.typed, self.suffix);
        let until = Instant::now() + SETTLE;
        loop {
            validate()?;
            self.focused()?;
            let current = self.reader.text();
            validate()?;
            self.focused()?;
            if current.is_ok_and(|value| normalize(&value) == expected)
                && input::no_held_keys().is_ok()
            {
                self.pending = false;
                return Ok(());
            }
            if Instant::now() >= until {
                return Err(failed("Accepted Unicode text delivery is unconfirmed"));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn focused(&self) -> Result<(), Error> {
        if unsafe { GetForegroundWindow() } != self.window {
            return Err(failed(
                "Foreground changed before Unicode delivery acknowledgement",
            ));
        }
        let focused = unsafe { self.uia.GetFocusedElement() }.map_err(failed)?;
        if !unsafe { self.uia.CompareElements(&focused, &self.element) }
            .map_err(failed)?
            .as_bool()
        {
            return Err(failed(
                "Focused control changed before Unicode delivery acknowledgement",
            ));
        }
        Ok(())
    }
}

impl Drop for TextDelivery {
    fn drop(&mut self) {
        if self.pending {
            // Do not release the shared input lease as clean. The worker is
            // reaped and a queued independent owner sees the abandoned lease.
            input::fail_delivery();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_utf16_is_never_lossily_acknowledged() {
        assert!(strict_text(&BSTR::from_wide(&[0xd83d]).unwrap()).is_err());
        assert_eq!(
            strict_text(&BSTR::from_wide(&[0xd83d, 0xde00]).unwrap()).unwrap(),
            "😀"
        );
        assert_eq!(normalize("a\r\nb\rc"), "a\nb\nc");
    }
}

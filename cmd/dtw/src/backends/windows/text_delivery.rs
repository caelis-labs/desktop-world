//! Read-only acknowledgement of focused text entry.
//! Supplementary scalars are read back before sending the next scalar.
//! A failed acknowledgement never repairs or replays text.
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

// Explicit local diagnostic opt-in: field contents are sensitive even in hex.
// Never read additional provider properties or emit contents by default.
fn tracing() -> bool {
    std::env::var_os("DTW_WINDOWS_TEXT_TRACE").is_some_and(|value| value == "1")
}
fn trace_units(label: &str, units: &[u16]) {
    if tracing() {
        eprintln!(
            "DTW_WINDOWS_TEXT_TRACE {label}: units={} first_128={:04x?} truncated={}",
            units.len(),
            &units[..units.len().min(128)],
            units.len() > 128
        );
    }
}
fn trace_gate(label: &str, passed: bool) -> bool {
    if tracing() {
        eprintln!("DTW_WINDOWS_TEXT_TRACE empty_proxy_v2 {label}={passed}");
    }
    passed
}
fn trace_text(label: &str, value: &str) {
    if tracing() {
        trace_units(label, &value.encode_utf16().collect::<Vec<_>>());
    }
}

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
    Value(IUIAutomationValuePattern),
    Edit(super::edit_delivery::Edit),
}
impl Reader {
    fn text(&self) -> Result<String, Error> {
        match self {
            Self::Text(pattern) => unsafe { pattern.DocumentRange() }
                .map_err(failed)
                .and_then(|range| text(&range)),
            Self::Value(pattern) => value_text(pattern),
            Self::Edit(edit) => edit.text(),
        }
    }
}

fn text(range: &IUIAutomationTextRange) -> Result<String, Error> {
    let result = unsafe { range.GetText(MAX_UNITS + 1) };
    if tracing() {
        if let Err(error) = &result {
            eprintln!("DTW_WINDOWS_TEXT_TRACE GetText failed: {error:?}");
        }
    }
    let raw = result.map_err(failed)?;
    trace_units("GetText", raw.as_wide());
    if raw.len() > MAX_UNITS as usize {
        return Err(failed(
            "Unicode delivery text exceeds the bounded read budget",
        ));
    }
    strict_text(&raw)
}
fn value_text(pattern: &IUIAutomationValuePattern) -> Result<String, Error> {
    let raw = unsafe { pattern.CurrentValue() }.map_err(failed)?;
    if raw.len() > MAX_UNITS as usize {
        return Err(failed(
            "Unicode delivery value exceeds the bounded read budget",
        ));
    }
    trace_units("value_readback", raw.as_wide());
    strict_text(&raw)
}

// An accessible label or embedded-object marker is not editable content when
// the same control independently and repeatedly reports an empty value. A
// literal label/marker in ValuePattern must never be removed.
fn empty_proxy(document: &str, name: &str, first_value: &str, last_value: &str) -> bool {
    !document.is_empty()
        && (document == name || document == "\u{fffc}")
        && first_value.is_empty()
        && last_value.is_empty()
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
            if tracing() {
                eprintln!(
                    "DTW_WINDOWS_TEXT_TRACE admission: window={window:?} process={:?} control_type={:?} native_window={:?}",
                    element.CurrentProcessId(), element.CurrentControlType(), element.CurrentNativeWindowHandle()
                );
            }
            match super::edit_delivery::Edit::new(window) {
                Ok(edit) => {
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
                }
                Err(Error::UnsupportedCapability) => {}
                Err(error) => return Err(error),
            }
            let pattern = super::native::optional_pattern::<IUIAutomationTextPattern>(
                &element,
                UIA_TextPatternId,
            )?
            .ok_or(Error::UnsupportedCapability)?;
            let document = pattern.DocumentRange().map_err(failed)?;
            let full = text(&document)?;
            trace_text("document", &full);
            let ranges = pattern.GetSelection().map_err(failed)?;
            let range_count = ranges.Length().map_err(failed)?;
            if tracing() {
                eprintln!("DTW_WINDOWS_TEXT_TRACE selection_count={range_count}");
            }
            if range_count != 1 {
                return Err(Error::UnsupportedCapability);
            }
            let selection = ranges.GetElement(0).map_err(failed)?;
            selection_owned(&uia, &selection, &element)?;
            if tracing() {
                eprintln!(
                    "DTW_WINDOWS_TEXT_TRACE endpoints: start_vs_document_start={:?} end_vs_document_end={:?} start_vs_end={:?}",
                    selection.CompareEndpoints(TextPatternRangeEndpoint_Start, &document, TextPatternRangeEndpoint_Start),
                    selection.CompareEndpoints(TextPatternRangeEndpoint_End, &document, TextPatternRangeEndpoint_End),
                    selection.CompareEndpoints(TextPatternRangeEndpoint_Start, &selection, TextPatternRangeEndpoint_End)
                );
                match super::native::optional_pattern::<IUIAutomationValuePattern>(
                    &element,
                    UIA_ValuePatternId,
                ) {
                    Ok(Some(value)) => {
                        eprintln!(
                            "DTW_WINDOWS_TEXT_TRACE ValuePattern readonly={:?}",
                            value.CurrentIsReadOnly()
                        );
                        match value.CurrentValue() {
                            Ok(raw) => trace_units("value", raw.as_wide()),
                            Err(error) => {
                                eprintln!("DTW_WINDOWS_TEXT_TRACE CurrentValue failed: {error:?}")
                            }
                        }
                    }
                    Ok(None) => eprintln!("DTW_WINDOWS_TEXT_TRACE no ValuePattern"),
                    Err(error) => {
                        eprintln!("DTW_WINDOWS_TEXT_TRACE ValuePattern failed: {error:?}")
                    }
                }
            }
            let selected = text(&selection)?;
            // Chromium's empty Edit TextPattern can expose the accessible name
            // instead of content, with non-partitioning ranges (e.g. a 20-unit
            // label but a one-unit suffix). Establish emptiness independently
            // before cloning/moving ranges: Chromium can normalize the empty
            // document's label range to a single character during those calls.
            // Verify both the untouched and a fresh document range instead.
            // Ownership was proved above; require a collapsed, empty selection
            // on an Edit and stable, writable ValuePattern plus proxy identity.
            if trace_gate("document_nonempty", !full.is_empty())
                && trace_gate("selected_empty", selected.is_empty())
                && trace_gate(
                    "is_edit",
                    element.CurrentControlType().map_err(failed)? == UIA_EditControlTypeId,
                )
                && trace_gate(
                    "selection_collapsed",
                    selection
                        .CompareEndpoints(
                            TextPatternRangeEndpoint_Start,
                            &selection,
                            TextPatternRangeEndpoint_End,
                        )
                        .map_err(failed)?
                        == 0,
                )
            {
                if let Some(value) = super::native::optional_pattern::<IUIAutomationValuePattern>(
                    &element,
                    UIA_ValuePatternId,
                )? {
                    if trace_gate(
                        "first_writable",
                        !value.CurrentIsReadOnly().map_err(failed)?.as_bool(),
                    ) {
                        let first_value = value_text(&value)?;
                        if trace_gate("first_value_empty", first_value.is_empty()) {
                            let name = strict_text(&element.CurrentName().map_err(failed)?)?;
                            trace_text("proxy_first_name", &name);
                            if trace_gate(
                                "first_proxy_identity",
                                empty_proxy(&full, &name, &first_value, &first_value),
                            ) && trace_gate("document_stable", text(&document)? == full)
                                && trace_gate(
                                    "fresh_document_stable",
                                    text(&pattern.DocumentRange().map_err(failed)?)? == full,
                                )
                                && {
                                    let latest = pattern.GetSelection().map_err(failed)?;
                                    if latest.Length().map_err(failed)? != 1 {
                                        return Err(Error::UnsupportedCapability);
                                    }
                                    let latest = latest.GetElement(0).map_err(failed)?;
                                    selection_owned(&uia, &latest, &element)?;
                                    trace_gate(
                                        "fresh_selection_collapsed",
                                        latest
                                            .CompareEndpoints(
                                                TextPatternRangeEndpoint_Start,
                                                &latest,
                                                TextPatternRangeEndpoint_End,
                                            )
                                            .map_err(failed)?
                                            == 0,
                                    ) && trace_gate(
                                        "fresh_selection_empty",
                                        text(&latest)?.is_empty(),
                                    )
                                }
                                && {
                                    let last_name =
                                        strict_text(&element.CurrentName().map_err(failed)?)?;
                                    trace_text("proxy_last_name", &last_name);
                                    trace_gate("name_stable", last_name == name)
                                }
                                && trace_gate(
                                    "last_writable",
                                    !value.CurrentIsReadOnly().map_err(failed)?.as_bool(),
                                )
                                && trace_gate(
                                    "last_proxy_identity",
                                    empty_proxy(&full, &name, &first_value, &value_text(&value)?),
                                )
                            {
                                // Keep using the independent content reader for
                                // acknowledgement: a label is never delivery proof.
                                return Ok(Self {
                                    uia,
                                    element,
                                    reader: Reader::Value(value),
                                    window,
                                    prefix: String::new(),
                                    suffix: String::new(),
                                    typed: String::new(),
                                    pending: false,
                                    partial: false,
                                });
                            }
                        }
                    }
                }
            }
            // Chrome can order a field's terminal caret after DocumentRange's
            // end even though both represent the same text offset. Prove the
            // selection's control ownership and exact text partition instead
            // of treating provider endpoint order as containment evidence.
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
            trace_text("prefix", &prefix);
            trace_text("selected", &selected);
            trace_text("suffix", &suffix);
            if tracing() {
                eprintln!(
                    "DTW_WINDOWS_TEXT_TRACE partition_matches={}",
                    format!("{prefix}{selected}{suffix}") == full
                );
            }
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
                return Err(Error::NativeFailure {
                    code: "text_delivery_unconfirmed".into(),
                    message: "The focused control did not confirm the exact typed text. Read the field before continuing or retrying.".into(),
                });
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    pub(super) fn focused(&self) -> Result<(), Error> {
        if unsafe { GetForegroundWindow() } != self.window {
            return Err(failed(
                "Foreground changed before Unicode delivery acknowledgement",
            ));
        }
        let focused = unsafe { self.uia.GetFocusedElement() }.map_err(failed)?;
        require_focus(&self.uia, &self.element, &focused)
    }
}

pub(super) fn require_focus(
    uia: &IUIAutomation,
    expected: &IUIAutomationElement,
    actual: &IUIAutomationElement,
) -> Result<(), Error> {
    unsafe {
        if expected
            .CurrentHasKeyboardFocus()
            .is_ok_and(|value| value.as_bool())
            && uia
                .CompareElements(expected, actual)
                .map_err(failed)?
                .as_bool()
        {
            return Ok(());
        }
        let role = actual
            .CurrentControlType()
            .ok()
            .map(super::native::role)
            .unwrap_or("control");
        let name = actual.CurrentName().ok().map(|name| {
            name.to_string()
                .chars()
                .filter(|ch| !ch.is_control())
                .take(100)
                .collect::<String>()
        });
        let field = name
            .filter(|name| !name.is_empty())
            .map_or_else(|| role.to_owned(), |name| format!("{role} {name:?}"));
        Err(Error::NativeFailure {
            code: "text_focus_mismatch".into(),
            message: format!("The intended text field was not focused. Focus was on {field}. Observe the app before continuing."),
        })
    }
}

fn selection_owned(
    uia: &IUIAutomation,
    range: &IUIAutomationTextRange,
    focused: &IUIAutomationElement,
) -> Result<(), Error> {
    unsafe {
        let walker = uia.RawViewWalker().map_err(failed)?;
        let mut owner = range.GetEnclosingElement().map_err(failed)?;
        for depth in 0..128 {
            if tracing() {
                eprintln!("DTW_WINDOWS_TEXT_TRACE selection_owner depth={depth} process={:?} control_type={:?}", owner.CurrentProcessId(), owner.CurrentControlType());
            }
            if uia
                .CompareElements(&owner, focused)
                .map_err(failed)?
                .as_bool()
            {
                if tracing() {
                    eprintln!("DTW_WINDOWS_TEXT_TRACE selection_owned=true depth={depth}");
                }
                return Ok(());
            }
            owner = walker.GetParentElement(&owner).map_err(failed)?;
        }
        Err(Error::InvalidObservation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_chromium_label_proxy_requires_independent_empty_values() {
        // Captured Chrome 156: full label, collapsed owned selection, and
        // prefix/selection empty but suffix "A". Text ranges do not partition
        // content; ValuePattern supplies the independent empty-content proof.
        let label = "Audit Unicode editor";
        assert_ne!(format!("{}{}{}", "", "", "A"), label);
        assert!(empty_proxy(label, label, "", ""));
        assert!(empty_proxy("\u{fffc}", label, "", ""));
        assert!(!empty_proxy("different text", label, "", ""));
        assert!(!empty_proxy("", "", "", ""));
    }

    #[test]
    fn literal_labels_markers_and_value_changes_are_not_empty_proxies() {
        for literal in ["Audit Unicode editor", "\u{fffc}", "😀", "line\nlabel"] {
            assert!(!empty_proxy(literal, literal, literal, literal));
            assert!(!empty_proxy(literal, literal, "", literal));
            assert!(!empty_proxy(literal, literal, literal, ""));
        }
    }

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

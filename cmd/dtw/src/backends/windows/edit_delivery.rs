//! Bounded read-only text/selection samples for native Win32 Edit controls.
//! These standard messages are below WM_USER and use system marshalling.
use crate::{backends::native_client::failed, computer::Error};
use windows::Win32::{Foundation::*, UI::WindowsAndMessaging::*};

pub(super) struct Edit(HWND, HWND);
impl Edit {
    pub(super) fn new(window: HWND) -> Result<Self, Error> {
        let mut gui = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetGUIThreadInfo(GetWindowThreadProcessId(window, None), &mut gui) }
            .map_err(failed)?;
        let mut name = [0u16; 256];
        let len = unsafe { GetClassNameW(gui.hwndFocus, &mut name) };
        let name = String::from_utf16_lossy(&name[..len.max(0) as usize]);
        if !(name.eq_ignore_ascii_case("Edit") || name.starts_with("WindowsForms10.EDIT."))
            || !unsafe { IsWindowUnicode(gui.hwndFocus) }.as_bool()
            || unsafe { GetWindowLongW(gui.hwndFocus, GWL_STYLE) } & (0x20 | 0x800) != 0 // ES_PASSWORD | ES_READONLY
            || unsafe { GetAncestor(gui.hwndFocus, GA_ROOT) } != window
        {
            return Err(Error::UnsupportedCapability);
        }
        Ok(Self(gui.hwndFocus, window))
    }
    fn validate(&self) -> Result<(), Error> {
        let mut gui = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetGUIThreadInfo(GetWindowThreadProcessId(self.1, None), &mut gui) }
            .map_err(failed)?;
        if gui.hwndFocus != self.0 || unsafe { GetAncestor(self.0, GA_ROOT) } != self.1 {
            return Err(Error::TargetChanged);
        }
        let (mut child, mut root) = (0, 0);
        unsafe {
            GetWindowThreadProcessId(self.0, Some(&mut child));
            GetWindowThreadProcessId(self.1, Some(&mut root));
        }
        if child == 0 || child != root {
            return Err(Error::TargetChanged);
        }
        Ok(())
    }
    fn message(&self, message: u32, w: WPARAM, l: LPARAM) -> Result<usize, Error> {
        self.validate()?;
        let mut reply = 0;
        let result = unsafe {
            SendMessageTimeoutW(
                self.0,
                message,
                w,
                l,
                SMTO_BLOCK | SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                50,
                Some(&mut reply),
            )
        };
        if result.0 == 0 {
            return Err(failed(
                "Native Edit acknowledgement read failed or timed out",
            ));
        }
        self.validate()?;
        Ok(reply)
    }
    pub(super) fn text(&self) -> Result<String, Error> {
        let len = self.message(WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0))?;
        if len >= 65_535 {
            return Err(Error::UnsupportedCapability);
        }
        let mut buffer = vec![0u16; len + 1];
        let copied = self.message(
            WM_GETTEXT,
            WPARAM(buffer.len()),
            LPARAM(buffer.as_mut_ptr() as isize),
        )?;
        if copied != len || self.message(WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0))? != len {
            return Err(Error::InvalidObservation);
        }
        String::from_utf16(&buffer[..copied])
            .map_err(|_| failed("Native Edit contains incomplete UTF-16"))
    }
    pub(super) fn selection(&self) -> Result<(String, String), Error> {
        let full = self.text()?;
        let selection = self.message(0x00b0, WPARAM(0), LPARAM(0))? as u32; // EM_GETSEL
        if selection == u32::MAX {
            return Err(Error::UnsupportedCapability);
        }
        let (start, end) = ((selection & 0xffff) as usize, (selection >> 16) as usize);
        if self.text()? != full {
            return Err(Error::InvalidObservation);
        }
        selection_parts(&full, start, end)
    }
}
fn selection_parts(full: &str, start: usize, end: usize) -> Result<(String, String), Error> {
    let units: Vec<_> = full.encode_utf16().collect();
    if start > end || end > units.len() {
        return Err(Error::InvalidObservation);
    }
    let prefix = String::from_utf16(&units[..start]).map_err(failed)?;
    let suffix = String::from_utf16(&units[end..]).map_err(failed)?;
    Ok((prefix, suffix))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_uses_utf16_offsets_without_splitting_existing_scalars() {
        assert_eq!(
            selection_parts("A😀BC🐦D", 3, 5).unwrap(),
            ("A😀".into(), "🐦D".into())
        );
        assert!(selection_parts("A😀B", 2, 3).is_err());
        assert!(selection_parts("A😀B", 1, 2).is_err());
        assert!(selection_parts("A😀B", 4, 3).is_err());
        assert!(selection_parts("A😀B", 0, 5).is_err());
    }
}

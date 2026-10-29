//! Foreground segments contain their complete key/button releases. Partial
//! insertion cleans only the confirmed held prefix before returning unknown.
use crate::backends::input_sequence::{
    normalized_text, valid_text, AdmissionError, Mutation, MutationGate,
};
use crate::backends::native_client::{check, failed};
use crate::computer::Error;
use std::time::{Duration, Instant};
use windows::{
    core::w,
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
        UI::{
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow},
        },
    },
};
static GATE: MutationGate = MutationGate::new();
pub fn close() {
    GATE.close();
}
pub fn cleanup_failed() -> bool {
    GATE.cleanup_failed()
}
pub fn closing() -> bool {
    GATE.closing()
}
pub fn mutation(until: Instant) -> Result<Mutation<'static>, Error> {
    GATE.admit_before(until).map_err(|error| match error {
        AdmissionError::Expired => Error::TimedOut,
        AdmissionError::Closed => Error::Closed,
    })
}
pub fn quiesce(budget: Duration) -> bool {
    GATE.quiesce(budget)
}
// On an uncertain release retain the handle until the owning process exits.
unsafe fn release_handle(handle: HANDLE) {
    if ReleaseMutex(handle).is_err() {
        GATE.fail_cleanup();
        return;
    }
    if CloseHandle(handle).is_err() {
        GATE.fail_cleanup();
    }
}
pub struct Lease(HANDLE);
impl Lease {
    pub fn acquire(until: Instant) -> Result<Self, Error> {
        unsafe {
            let handle = CreateMutexW(None, false, w!("Local\\DesktopWorld.SharedInput.v1"))
                .map_err(failed)?;
            loop {
                if let Err(error) = check(until) {
                    let _ = CloseHandle(handle);
                    return Err(error);
                }
                if closing() {
                    let _ = CloseHandle(handle);
                    return Err(Error::Closed);
                }
                let status = WaitForSingleObject(handle, 5);
                if status == WAIT_ABANDONED {
                    release_handle(handle);
                    GATE.fail_cleanup();
                    return Err(failed(
                        "Previous input owner was abandoned; its cleanup is unconfirmed",
                    ));
                }
                if status != WAIT_OBJECT_0 && status != WAIT_TIMEOUT {
                    let error = std::io::Error::last_os_error();
                    let _ = CloseHandle(handle);
                    return Err(failed(error));
                }
                if status == WAIT_OBJECT_0 {
                    if closing() || cleanup_failed() {
                        release_handle(handle);
                        return Err(Error::Closed);
                    }
                    return Ok(Self(handle));
                }
            }
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        // Failed paired cleanup still owns this mutex; the supervisor reaps its
        // process before another owner may acquire it. Do not close the handle.
        if !cleanup_failed() {
            unsafe {
                release_handle(self.0);
            }
        }
    }
}
pub fn no_held_keys() -> Result<(), Error> {
    for code in 1..=255 {
        if unsafe { GetAsyncKeyState(code) } < 0 {
            return Err(Error::Busy);
        }
    }
    Ok(())
}
pub fn focus(hwnd: HWND, until: Instant, dispatched: &mut bool) -> Result<(), Error> {
    unsafe {
        if GetForegroundWindow() != hwnd {
            let _mutation = mutation(until)?;
            *dispatched = true;
            let _ = SetForegroundWindow(hwnd);
        }
        if GetForegroundWindow() != hwnd {
            return Err(failed(
                "Target did not become foreground; no keyboard input issued",
            ));
        }
        Ok(())
    }
}
fn key(vk: u16, scan: u16, mut flags: KEYBD_EVENT_FLAGS) -> INPUT {
    if matches!(vk, 33..=40 | 45 | 46 | 91 | 92) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}
pub fn keyboard(text: Option<&str>, keys: Option<&[String]>) -> Result<Vec<INPUT>, Error> {
    let mut batch = Vec::new();
    if let Some(text) = text {
        if !valid_text(text, false) {
            return Err(failed("Text exceeds the UTF-8/encoded input contract"));
        }
        let units: Vec<_> = normalized_text(text).encode_utf16().collect();
        for unit in units {
            if unit == 10 || unit == 13 {
                batch.push(key(13, 0, KEYBD_EVENT_FLAGS(0)));
                batch.push(key(13, 0, KEYEVENTF_KEYUP));
            } else {
                batch.push(key(0, unit, KEYEVENTF_UNICODE));
                batch.push(key(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
            }
        }
    } else if let Some(keys) = keys {
        if keys.is_empty() || keys.len() > 16 {
            return Err(failed("Key chord needs 1..16 distinct keys"));
        }
        let mut codes = Vec::new();
        for name in keys {
            let code = keycode(name).ok_or(Error::UnsupportedCapability)?;
            if codes.contains(&code) {
                return Err(failed("Duplicate chord key"));
            }
            codes.push(code);
        }
        for code in &codes {
            batch.push(key(*code, 0, KEYBD_EVENT_FLAGS(0)));
        }
        for code in codes.iter().rev() {
            batch.push(key(*code, 0, KEYEVENTF_KEYUP));
        }
    }
    Ok(batch)
}
// Keep surrogate pairs within one indivisible scalar segment. A chord is one
// segment as well: its reverse releases must not be interrupted by target checks.
pub fn keyboard_segments(
    text: Option<&str>,
    keys: Option<&[String]>,
) -> Result<Vec<Vec<INPUT>>, Error> {
    if let Some(text) = text {
        if !valid_text(text, false) {
            return Err(failed("Text exceeds the UTF-8/encoded input contract"));
        }
        normalized_text(text)
            .chars()
            .map(|scalar| keyboard(Some(&scalar.to_string()), None))
            .collect()
    } else {
        Ok(vec![keyboard(None, keys)?])
    }
}
pub fn keycode(name: &str) -> Option<u16> {
    let upper = name.to_ascii_uppercase();
    Some(match upper.as_str() {
        "CTRL" | "CONTROL" => 17,
        "SHIFT" => 16,
        "ALT" | "OPTION" => 18,
        "META" | "SUPER" | "WIN" => 91,
        "ENTER" | "RETURN" => 13,
        "TAB" => 9,
        "ESC" | "ESCAPE" => 27,
        "SPACE" => 32,
        "BACKSPACE" => 8,
        "DELETE" => 46,
        "LEFT" => 37,
        "UP" => 38,
        "RIGHT" => 39,
        "DOWN" => 40,
        "HOME" => 36,
        "END" => 35,
        "PAGEUP" | "PAGE_UP" => 33,
        "PAGEDOWN" | "PAGE_DOWN" => 34,
        _ => {
            if upper.len() == 1 && upper.as_bytes()[0].is_ascii_alphanumeric() {
                upper.as_bytes()[0] as u16
            } else if let Some(n) = upper
                .strip_prefix('F')
                .and_then(|s| s.parse::<u16>().ok())
                .filter(|n| (1..=24).contains(n))
            {
                111 + n
            } else {
                return None;
            }
        }
    })
}
pub fn mouse(flags: MOUSE_EVENT_FLAGS, data: u32, dx: i32, dy: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}
pub fn send(
    batch: &[INPUT],
    hwnd: HWND,
    until: Instant,
    dispatched: &mut bool,
) -> Result<(), Error> {
    if batch.is_empty() {
        return Ok(());
    }
    if unsafe { GetForegroundWindow() } != hwnd {
        return Err(failed("Foreground changed before dispatch"));
    }
    let _mutation = mutation(until)?;
    *dispatched = true;
    let inserted = unsafe { SendInput(batch, std::mem::size_of::<INPUT>() as i32) };
    if inserted != batch.len() as u32 {
        let releases = pending_releases(&batch[..(inserted as usize).min(batch.len())]);
        if !releases.is_empty() {
            let released = unsafe { SendInput(&releases, std::mem::size_of::<INPUT>() as i32) };
            if released != releases.len() as u32 {
                GATE.fail_cleanup();
                return Err(failed("Input insertion and paired cleanup incomplete"));
            }
        }
        return Err(failed(
            "Input batch only partially inserted; effects unknown",
        ));
    }
    if unsafe { GetForegroundWindow() } != hwnd {
        return Err(failed("Foreground changed after dispatch; effects unknown"));
    }
    Ok(())
}

// SendInput reports the inserted prefix. Release only keys/buttons which that
// prefix actually left down; never synthesize ups for events not submitted.
fn pending_releases(prefix: &[INPUT]) -> Vec<INPUT> {
    let mut held: Vec<(u8, u16, u16, INPUT)> = Vec::new();
    for event in prefix {
        unsafe {
            if event.r#type == INPUT_KEYBOARD {
                let k = event.Anonymous.ki;
                if k.dwFlags.contains(KEYEVENTF_KEYUP) {
                    if let Some(index) = held.iter().rposition(|(kind, vk, scan, _)| {
                        *kind == 0 && *vk == k.wVk.0 && *scan == k.wScan
                    }) {
                        held.remove(index);
                    }
                } else {
                    held.push((
                        0,
                        k.wVk.0,
                        k.wScan,
                        key(k.wVk.0, k.wScan, k.dwFlags | KEYEVENTF_KEYUP),
                    ));
                }
            } else if event.r#type == INPUT_MOUSE {
                let flags = event.Anonymous.mi.dwFlags;
                for (kind, down, up) in [
                    (1, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                    (2, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                ] {
                    if flags.contains(up) {
                        if let Some(index) = held.iter().rposition(|(k, _, _, _)| *k == kind) {
                            held.remove(index);
                        }
                    }
                    if flags.contains(down) {
                        held.push((kind, 0, 0, mouse(up, 0, 0, 0)));
                    }
                }
            }
        }
    }
    held.into_iter()
        .rev()
        .map(|(_, _, _, release)| release)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_surrogates_and_newlines_have_balanced_releases() {
        let batch = keyboard(Some("中😀\r\n"), None).unwrap();
        assert_eq!(batch.len(), 8);
        for pair in batch.chunks_exact(2) {
            unsafe {
                let down = pair[0].Anonymous.ki;
                let up = pair[1].Anonymous.ki;
                assert_eq!((down.wVk, down.wScan), (up.wVk, up.wScan));
                assert!(!down.dwFlags.contains(KEYEVENTF_KEYUP));
                assert!(up.dwFlags.contains(KEYEVENTF_KEYUP));
            }
        }
        assert_eq!(unsafe { batch[6].Anonymous.ki.wVk }, VIRTUAL_KEY(13));
    }
    #[test]
    fn text_segments_keep_one_scalar_and_complete_chord_together() {
        let segments = keyboard_segments(Some("中😀\r\n"), None).unwrap();
        assert_eq!(
            segments.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![2, 4, 2]
        );
        assert_eq!(
            keyboard_segments(Some(&"a".repeat(16384)), None)
                .unwrap()
                .len(),
            16384
        );
        assert!(keyboard_segments(Some(&"a".repeat(16385)), None).is_err());
        let chord = keyboard_segments(None, Some(&["CTRL".into(), "A".into()])).unwrap();
        assert_eq!(chord.len(), 1);
        assert_eq!(chord[0].len(), 4);
    }
    #[test]
    fn partial_batch_releases_only_its_inserted_held_prefix() {
        let chord = keyboard(None, Some(&["CTRL".into(), "A".into()])).unwrap();
        assert!(pending_releases(&chord[..0]).is_empty());
        assert_eq!(pending_releases(&chord[..1]).len(), 1);
        assert_eq!(pending_releases(&chord[..2]).len(), 2);
        let remaining = pending_releases(&chord[..3]);
        assert_eq!(remaining.len(), 1);
        assert_eq!(unsafe { remaining[0].Anonymous.ki.wVk }, VIRTUAL_KEY(17));
        assert!(pending_releases(&chord).is_empty());
        let text = keyboard(Some("ab"), None).unwrap();
        assert!(pending_releases(&text[..2]).is_empty());
        let partial = pending_releases(&text[..3]);
        assert_eq!(partial.len(), 1);
        assert_eq!(unsafe { partial[0].Anonymous.ki.wScan }, 'b' as u16);
    }
    #[test]
    fn chord_is_validated_before_any_input_is_constructed() {
        assert!(keyboard(None, Some(&["CTRL".into(), "Control".into()])).is_err());
        assert!(keyboard(None, Some(&["CTRL".into(), "UNKNOWN".into()])).is_err());
        let batch = keyboard(None, Some(&["CTRL".into(), "SHIFT".into(), "A".into()])).unwrap();
        let codes: Vec<_> = batch
            .iter()
            .map(|input| unsafe { input.Anonymous.ki.wVk.0 })
            .collect();
        assert_eq!(codes, vec![17, 16, 65, 65, 16, 17]);
    }
}

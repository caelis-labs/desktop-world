//! Current-desktop inventory and read-only worker diagnostics. This module never
//! switches desktops, modifies their ACLs or attempts to escape a host sandbox.
use crate::computer::Error;
use serde_json::{json, Value};
use windows::Win32::{
    Foundation::{
        CloseHandle, GetLastError, SetLastError, BOOL, ERROR_MORE_DATA, ERROR_SUCCESS, HANDLE,
        HWND, LPARAM,
    },
    Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenHasRestrictions,
        TokenIntegrityLevel, TokenIsAppContainer, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    },
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        StationsAndDesktops::{
            CloseDesktop, GetProcessWindowStation, GetThreadDesktop, GetUserObjectInformationW,
            OpenInputDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, DESKTOP_HOOKCONTROL,
            DESKTOP_READOBJECTS, UOI_NAME,
        },
        Threading::{
            GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, WNDENUMPROC},
};

const WINDOW_LIMIT: usize = 4096;
// The generated EnumWindows binding converts FALSE to a Result immediately.
// Keep the documented BOOL ABI here so an unspecified LastError (including 0)
// and our own callback stop cannot become a misleading "operation succeeded".
#[link(name = "user32")]
extern "system" {
    #[link_name = "EnumWindows"]
    fn enum_windows(callback: WNDENUMPROC, param: LPARAM) -> BOOL;
}

#[derive(Default)]
struct Inventory {
    windows: Vec<HWND>,
    callbacks: usize,
    capped: bool,
}
impl Inventory {
    fn push(&mut self, window: HWND) -> bool {
        self.callbacks += 1;
        if self.windows.len() == WINDOW_LIMIT {
            self.capped = true;
            false
        } else {
            self.windows.push(window);
            true
        }
    }
}
unsafe extern "system" fn enum_window(window: HWND, param: LPARAM) -> BOOL {
    let state = &mut *(param.0 as *mut Inventory);
    if state.push(window) {
        BOOL(1)
    } else {
        // EnumWindows documents LastError as the callback's responsibility when
        // the callback returns FALSE. The explicit stop reason remains primary.
        SetLastError(ERROR_MORE_DATA);
        BOOL(0)
    }
}
pub(super) fn windows() -> Result<Vec<HWND>, Error> {
    let mut state = Inventory::default();
    let (returned, last_error) = unsafe {
        SetLastError(ERROR_SUCCESS);
        let returned = enum_windows(Some(enum_window), LPARAM(&mut state as *mut _ as isize));
        (returned.as_bool(), GetLastError().0)
    };
    if std::env::var("DTW_AX_READINESS_TRACE").ok().as_deref() == Some("1") {
        eprintln!(
            "DTW Windows enumeration: {}",
            json!({"raw_return":u8::from(returned),"last_error":last_error,
                "callbacks":state.callbacks,"capped":state.capped,"context":context()})
        );
    }
    finish_inventory(state, returned, last_error, context)
}
fn finish_inventory(
    state: Inventory,
    returned: bool,
    last_error: u32,
    diagnostics: impl FnOnce() -> Value,
) -> Result<Vec<HWND>, Error> {
    let (code, reason) = if state.capped {
        (
            "desktop_inventory_limit",
            format!(
                "Desktop inventory exceeds {WINDOW_LIMIT} windows; callback stopped enumeration"
            ),
        )
    } else if returned {
        // Zero callbacks on a successful enumeration is a valid empty inventory
        // of this thread's desktop, not proof that the host has no open windows.
        return Ok(state.windows);
    } else if last_error == 0 {
        (
            "desktop_enumeration_unproven",
            "EnumWindows returned FALSE without a Win32 error; empty inventory is unproven".into(),
        )
    } else {
        (
            "desktop_enumeration_failed",
            format!("EnumWindows failed with Win32 error {last_error}"),
        )
    };
    Err(Error::NativeFailure {
        code: code.into(),
        message: format!(
            "{reason} (raw_return={}, last_error={last_error}, callbacks={}). Native worker context: {}. DTW enumerates its thread desktop; if the host isolates MCP from the interactive desktop, run MCP in a host-authorized interactive-user context. DTW did not switch desktops or change permissions.",
            u8::from(returned), state.callbacks, diagnostics()
        ),
    })
}

fn diagnostic<T>(value: windows::core::Result<T>, convert: impl FnOnce(T) -> Value) -> Value {
    match value {
        Ok(value) => convert(value),
        Err(error) => json!({"unavailable":if error.code().0 == 0 {
            "Native metadata read failed without an error code".into()
        } else {
            error.to_string()
        },"hresult":error.code().0}),
    }
}
unsafe fn object_name(handle: HANDLE) -> windows::core::Result<String> {
    // Desktop/station object names are bounded. No untrusted size controls an
    // allocation, and a failed/truncated read stays explicitly unavailable.
    let mut name = [0u16; 512];
    GetUserObjectInformationW(
        handle,
        UOI_NAME,
        Some(name.as_mut_ptr().cast()),
        std::mem::size_of_val(&name) as u32,
        None,
    )?;
    let len = name
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(name.len());
    Ok(String::from_utf16_lossy(&name[..len]))
}
unsafe fn token_flag(
    handle: HANDLE,
    class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Value {
    let mut flag = 0u32;
    let mut size = 0;
    diagnostic(
        GetTokenInformation(
            handle,
            class,
            Some((&mut flag as *mut u32).cast()),
            std::mem::size_of_val(&flag) as u32,
            &mut size,
        ),
        |_| json!(flag != 0),
    )
}
// A successful enumeration proves only the worker's desktop. New input also
// requires that desktop to be the current input desktop; never switch desktops.
pub(super) fn ensure_input_desktop() -> Result<(), Error> {
    let names = unsafe {
        (|| {
            let thread = object_name(HANDLE(GetThreadDesktop(GetCurrentThreadId())?.0))?;
            let input = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS)?;
            let name = object_name(HANDLE(input.0));
            let closed = CloseDesktop(input);
            let name = name?;
            closed?;
            Ok((thread, name))
        })()
    };
    validate_input_desktop(names)
}

unsafe fn integrity(process: HANDLE) -> Result<u32, Error> {
    use crate::backends::native_client::failed;
    let mut token = HANDLE::default();
    OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(failed)?;
    let mut buffer = [0usize; 32];
    let mut size = 0;
    let result = GetTokenInformation(
        token,
        TokenIntegrityLevel,
        Some(buffer.as_mut_ptr().cast()),
        std::mem::size_of_val(&buffer) as u32,
        &mut size,
    );
    let closed = CloseHandle(token);
    result.and(closed).map_err(failed)?;
    let label = &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
    let count = GetSidSubAuthorityCount(label.Label.Sid)
        .as_ref()
        .and_then(|count| count.checked_sub(1))
        .ok_or(Error::InvalidObservation)?;
    GetSidSubAuthority(label.Label.Sid, u32::from(count))
        .as_ref()
        .copied()
        .ok_or(Error::InvalidObservation)
}

/// GetAsyncKeyState can report all zeros on access denial. Require access to
/// the current desktop and a foreground process at or below our integrity.
pub(super) fn keyboard_state_owner() -> Result<HWND, Error> {
    use crate::backends::native_client::failed;
    ensure_input_desktop()?;
    unsafe {
        let desktop = OpenInputDesktop(
            DESKTOP_CONTROL_FLAGS(0),
            false,
            DESKTOP_ACCESS_FLAGS(DESKTOP_READOBJECTS.0 | DESKTOP_HOOKCONTROL.0),
        )
        .map_err(failed)?;
        CloseDesktop(desktop).map_err(failed)?;
        let foreground = GetForegroundWindow();
        let mut pid = 0;
        if GetWindowThreadProcessId(foreground, Some(&mut pid)) == 0 {
            return Err(Error::TargetChanged);
        }
        let own = integrity(GetCurrentProcess())?;
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(failed)?;
        let other = integrity(process);
        CloseHandle(process).map_err(failed)?;
        if other? > own || GetForegroundWindow() != foreground {
            return Err(Error::TargetChanged);
        }
        Ok(foreground)
    }
}
fn validate_input_desktop(names: windows::core::Result<(String, String)>) -> Result<(), Error> {
    let reason = match names {
        Ok((thread, input)) if !thread.is_empty() && thread == input => return Ok(()),
        Ok(_) => "Worker desktop is not the current input desktop".to_owned(),
        Err(error) => format!("Current input desktop could not be proved: {error}"),
    };
    Err(Error::NativeFailure {
        code: "desktop_unavailable".into(),
        message: format!("{reason}; new input stopped. Restore the authorized interactive desktop and read fresh state before another action."),
    })
}

pub(super) fn context() -> Value {
    unsafe {
        let pid = std::process::id();
        let thread_id = GetCurrentThreadId();
        let mut session = 0;
        let session_id = diagnostic(ProcessIdToSessionId(pid, &mut session), |_| json!(session));
        let window_station = diagnostic(
            GetProcessWindowStation().and_then(|handle| object_name(HANDLE(handle.0))),
            |name| json!(name),
        );
        let thread_desktop = diagnostic(
            GetThreadDesktop(thread_id).and_then(|handle| object_name(HANDLE(handle.0))),
            |name| json!(name),
        );
        let input_desktop = diagnostic(
            OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS).and_then(
                |handle| {
                    let name = object_name(HANDLE(handle.0));
                    let _ = CloseDesktop(handle);
                    name
                },
            ),
            |name| json!(name),
        );
        let mut token = HANDLE::default();
        let token = diagnostic(
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token),
            |_| {
                let result = json!({
                    "appcontainer":token_flag(token, TokenIsAppContainer),
                // This API reports filtering history, not the narrower
                // restricting-SID-list predicate of IsTokenRestricted.
                "filtered":token_flag(token, TokenHasRestrictions),
                });
                let _ = CloseHandle(token);
                result
            },
        );
        json!({"pid":pid,"thread_id":thread_id,"session_id":session_id,
            "window_station":window_station,"thread_desktop":thread_desktop,
            "input_desktop":input_desktop,"token":token})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_admission_requires_fresh_matching_desktop_metadata() {
        assert!(validate_input_desktop(Ok(("Default".into(), "Default".into()))).is_ok());
        for names in [
            Ok(("Default".into(), "Winlogon".into())),
            Ok((String::new(), String::new())),
            Err(windows::core::Error::from(windows::core::HRESULT(
                0x80070005u32 as i32,
            ))),
        ] {
            assert!(matches!(validate_input_desktop(names),
                Err(Error::NativeFailure { code, .. }) if code == "desktop_unavailable"));
        }
        // No sticky failure: a later action must acquire its own fresh proof.
        assert!(validate_input_desktop(Ok(("Default".into(), "Default".into()))).is_ok());
    }
    fn failure(state: Inventory, returned: bool, error: u32) -> (String, String) {
        match finish_inventory(
            state,
            returned,
            error,
            || json!({"thread_desktop":"fixture"}),
        ) {
            Err(Error::NativeFailure { code, message }) => (code, message),
            other => panic!("Expected explicit inventory failure, got {other:?}"),
        }
    }
    #[test]
    fn successful_empty_desktop_is_not_a_native_failure() {
        assert!(
            finish_inventory(Inventory::default(), true, 0, || panic!("not an error"))
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn unspecified_failure_is_not_silently_reported_as_empty_or_success() {
        let (code, message) = failure(Inventory::default(), false, 0);
        assert_eq!(code, "desktop_enumeration_unproven");
        assert!(message.contains("raw_return=0, last_error=0, callbacks=0"));
        assert!(message.contains("thread_desktop"));
        assert!(message.contains("host-authorized"));
    }
    #[test]
    fn failed_partial_enumeration_never_publishes_partial_inventory_as_complete() {
        let mut state = Inventory::default();
        assert!(state.push(HWND(1usize as *mut _)));
        let (code, message) = failure(state, false, 5);
        assert_eq!(code, "desktop_enumeration_failed");
        assert!(message.contains("Win32 error 5"));
        assert!(message.contains("callbacks=1"));
    }
    #[test]
    fn exact_budget_is_complete_but_callback_stop_has_its_own_reason() {
        let mut state = Inventory::default();
        for index in 0..WINDOW_LIMIT {
            assert!(state.push(HWND((index + 1) as *mut _)));
        }
        assert_eq!(
            finish_inventory(state, true, 0, || panic!("not an error"))
                .unwrap()
                .len(),
            WINDOW_LIMIT
        );
        let mut state = Inventory::default();
        for index in 0..=WINDOW_LIMIT {
            assert_eq!(
                state.push(HWND((index + 1) as *mut _)),
                index < WINDOW_LIMIT
            );
        }
        let (code, message) = failure(state, false, 0);
        assert_eq!(code, "desktop_inventory_limit");
        assert!(message.contains("exceeds 4096"));
        assert!(message.contains("callbacks=4097"));
    }
    #[test]
    fn stale_last_error_on_success_is_not_an_enumeration_failure() {
        let mut state = Inventory::default();
        assert!(state.push(HWND(1usize as *mut _)));
        assert_eq!(
            finish_inventory(state, true, 5, || panic!("not an error"))
                .unwrap()
                .len(),
            1
        );
    }
}

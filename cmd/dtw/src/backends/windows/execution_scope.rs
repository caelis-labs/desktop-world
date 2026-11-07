//! Script-owned foreground/cursor borrowing. The WinEvent listener observes
//! intermediate handoffs independently of worker IO; it never generates input.
use super::{input, native::process_stamp};
use crate::{
    backends::native_client::failed,
    computer::{AppIdentity, AppInstance, Error, FocusReturn},
};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{HMODULE, HWND, LPARAM, POINT, WPARAM},
    System::{SystemInformation::GetTickCount, Threading::GetCurrentThreadId},
    UI::{Accessibility::*, WindowsAndMessaging::*},
};

static ACTIVE: Mutex<Option<Scope>> = Mutex::new(None);
static SETTLEMENT: Mutex<()> = Mutex::new(());
static MONITOR_READY: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct Identity {
    hwnd: usize,
    thread: u32,
    app: AppInstance,
    proven: bool,
}
impl Identity {
    fn read(window: HWND) -> Result<Self, Error> {
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        if thread == 0 || !unsafe { IsWindow(window) }.as_bool() {
            return Err(Error::TargetChanged);
        }
        Ok(Self {
            hwnd: window.0 as usize,
            thread,
            app: process_stamp(pid)?,
            proven: true,
        })
    }
    fn live(&self) -> bool {
        self.proven
            && Self::read(hwnd(self.hwnd))
                .is_ok_and(|now| now.thread == self.thread && now.app == self.app)
    }
    fn original(window: HWND) -> Self {
        Self::read(window).unwrap_or_else(|_| {
            let mut pid = 0;
            let thread = unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
            Self {
                hwnd: window.0 as usize,
                thread,
                app: AppInstance {
                    identity: AppIdentity(String::new()),
                    pid,
                    generation: 0,
                },
                proven: false,
            }
        })
    }
}
fn hwnd(value: usize) -> HWND {
    HWND(value as *mut _)
}
fn root(window: HWND) -> HWND {
    let root = unsafe { GetAncestor(window, GA_ROOT) };
    if root.0.is_null() {
        window
    } else {
        root
    }
}
fn foreground() -> HWND {
    root(unsafe { GetForegroundWindow() })
}
fn cursor() -> Result<(i32, i32), Error> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.map_err(failed)?;
    Ok((point.x, point.y))
}
fn handoff_error() -> Error {
    Error::NativeFailure { code:"execution_focus_changed".into(), message:"Foreground ownership changed outside the selected application during this script; further input was refused. Observe current state and start a fresh execution.".into() }
}
fn observation_error() -> Error {
    Error::NativeFailure {code:"execution_scope_unproven".into(),message:"Windows input-scope observation became unavailable; no new payload was issued. Inspect state in a fresh execution before new input.".into()}
}
fn transition(
    borrowed: &mut bool,
    external: &mut bool,
    target: bool,
    original: bool,
    last_closed: bool,
    actual_target: bool,
) {
    if *external {
        return;
    }
    if target {
        *borrowed |= !original && actual_target;
    } else if !original || (*borrowed && !last_closed) {
        *external = true;
    }
}

struct Scope {
    app: AppInstance,
    original: Identity,
    start_tick: u32,
    last_foreground: Option<Identity>,
    expected: Identity,
    next: Option<Identity>,
    initial_windows: HashSet<usize>,
    destroyed: HashSet<usize>,
    parents: HashSet<usize>,
    modal_returns: HashSet<(usize, usize)>,
    borrowed: bool,
    external: bool,
    cursor_original: (i32, i32),
    cursor_last: (i32, i32),
    pointer_borrowed: bool,
    evidence_lost: bool,
    last_hidden: Option<usize>,
}
impl Scope {
    fn remember_parents(&mut self, window: HWND) {
        let mut owner = window;
        for _ in 0..128 {
            let Ok(parent) = (unsafe { GetWindow(owner, GW_OWNER) }) else {
                break;
            };
            if Identity::read(parent).map_or(true, |identity| identity.app != self.app) {
                break;
            }
            self.parents.insert(parent.0 as usize);
            if !unsafe { windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(parent) }
                .as_bool()
            {
                self.modal_returns
                    .insert((window.0 as usize, parent.0 as usize));
            }
            if parent == owner {
                self.evidence_lost = true;
                break;
            }
            owner = parent;
        }
    }
    fn expect_receiver(&mut self, window: HWND) -> Result<(), Error> {
        let identity = Identity::read(window)?;
        if identity.app != self.app || self.destroyed.contains(&identity.hwnd) {
            return Err(Error::TargetChanged);
        }
        self.next = Some(identity);
        self.remember_parents(window);
        Ok(())
    }
    fn observe(&mut self, window: HWND) {
        if self.external {
            return;
        }
        // WinForms UIA Invoke briefly reports the child BUTTON as foreground.
        // It is the same exact top-level receiver, not a peer-window handoff.
        // Keyboard admission still independently requires the actual HWND.
        let window = root(window);
        let value = window.0 as usize;
        // Hidden service/proxy windows are not evidence of a user's external
        // application choice. Payload admission still proves its exact visible
        // receiver; this never makes a hidden foreground a keyboard destination.
        if !unsafe { IsWindowVisible(window) }.as_bool() {
            if self.last_hidden != Some(value)
                && std::env::var("DTW_INPUT_TRACE").ok().as_deref() == Some("1")
            {
                eprintln!(
                    "DTW_WINDOWS_SCOPE hidden_foreground={} target_pid={}",
                    value, self.app.pid
                );
            }
            self.last_hidden = Some(value);
            return;
        }
        self.last_hidden = None;
        let identity = Identity::read(window).ok();
        let last_closed = self.last_foreground.as_ref().is_some_and(|last| {
            !last.live() || !unsafe { IsWindowVisible(hwnd(last.hwnd)) }.as_bool()
        });
        // WinForms re-enables an owned modal's parent before hiding/destroying
        // the dialog. This exact disabled-to-enabled owner relationship proves
        // the legitimate return without accepting a switch to a live peer.
        let modal_return = self.last_foreground.as_ref().is_some_and(|last| {
            self.modal_returns.contains(&(last.hwnd, value))
                && unsafe { windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(window) }
                    .as_bool()
        });
        let target = identity.as_ref().is_some_and(|identity| {
            identity.app == self.app
                && !self.destroyed.contains(&value)
                && (value == self.expected.hwnd
                    || self.next.as_ref().is_some_and(|next| next.hwnd == value)
                    || (last_closed && self.parents.contains(&value))
                    || modal_return
                    || !self.initial_windows.contains(&value))
        });
        // Closing the exact last receiver can let Windows return to the
        // original window. This permits settlement, never a different owner.
        transition(
            &mut self.borrowed,
            &mut self.external,
            target,
            value == self.original.hwnd,
            last_closed,
            foreground() == window,
        );
        if self.external && std::env::var("DTW_INPUT_TRACE").ok().as_deref() == Some("1") {
            eprintln!("DTW_WINDOWS_SCOPE external actual={} original={} expected={} target_pid={} actual_pid={:?}",value,self.original.hwnd,self.expected.hwnd,self.app.pid,identity.as_ref().map(|identity|identity.app.pid));
        }
        if target && !self.external {
            self.remember_parents(window);
            self.expected = identity.as_ref().unwrap().clone();
            if self.next.as_ref().is_some_and(|next| next.hwnd == value) {
                self.next = None;
            }
            self.initial_windows.insert(value);
            self.last_foreground = identity;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn away_and_return_never_reacquires_an_execution() {
        let (mut borrowed, mut external) = (false, false);
        transition(&mut borrowed, &mut external, false, true, false, false);
        assert!(!external);
        transition(&mut borrowed, &mut external, true, false, false, true);
        assert!(borrowed);
        transition(&mut borrowed, &mut external, false, false, false, false);
        transition(&mut borrowed, &mut external, true, false, false, true);
        assert!(external);
    }
    #[test]
    fn original_window_is_external_after_borrow_unless_exact_receiver_closed() {
        for last_closed in [false, true] {
            let (mut borrowed, mut external) = (true, false);
            transition(
                &mut borrowed,
                &mut external,
                false,
                true,
                last_closed,
                false,
            );
            assert_eq!(external, !last_closed);
            transition(&mut borrowed, &mut external, false, false, true, false);
            assert!(external, "a closed target cannot authorize a foreign owner");
        }
    }
    #[test]
    fn no_borrow_never_authorizes_a_new_foreign_foreground() {
        let (mut borrowed, mut external) = (false, false);
        transition(&mut borrowed, &mut external, false, false, false, false);
        transition(&mut borrowed, &mut external, false, true, false, false);
        assert!(external);
        assert!(!borrowed);
    }
    #[test]
    fn provider_notification_alone_does_not_prove_a_foreground_borrow() {
        let (mut borrowed, mut external) = (false, false);
        transition(&mut borrowed, &mut external, true, false, false, false);
        transition(&mut borrowed, &mut external, false, true, false, false);
        assert!(!borrowed);
        assert!(!external);
    }
}
unsafe extern "system" fn foreground_event(
    _: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    child: i32,
    _: u32,
    event_tick: u32,
) {
    if let Some(scope) = ACTIVE.lock().unwrap().as_mut() {
        // WinEvent timestamps are a wrapping millisecond clock. Queued events
        // preceding admission must not be attributed to this execution.
        if event_tick.wrapping_sub(scope.start_tick) as i32 >= 0 {
            if event == EVENT_OBJECT_DESTROY && object == 0 && child == 0 {
                if scope.destroyed.len() >= 4096 {
                    scope.evidence_lost = true;
                } else {
                    scope.destroyed.insert(window.0 as usize);
                }
            } else if event == EVENT_SYSTEM_FOREGROUND {
                if std::env::var("DTW_INPUT_TRACE").ok().as_deref() == Some("1") {
                    let mut class = [0u16; 256];
                    let length = unsafe { GetClassNameW(window, &mut class) };
                    let mut bounds = windows::Win32::Foundation::RECT::default();
                    let _ = unsafe { GetWindowRect(window, &mut bounds) };
                    eprintln!("DTW_WINDOWS_SCOPE event actual={} current={} tick={} start={} expected={} next={:?} class={} owner={:?} style={} bounds={:?}", window.0 as usize, unsafe { GetForegroundWindow() }.0 as usize, event_tick, scope.start_tick, scope.expected.hwnd, scope.next.as_ref().map(|next| next.hwnd), String::from_utf16_lossy(&class[..length.max(0) as usize]), unsafe { GetWindow(window, GW_OWNER) }.ok().map(|owner| owner.0 as usize), unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) }, bounds);
                }
                scope.observe(window);
            }
        }
    }
}
pub struct Monitor {
    thread: Option<JoinHandle<()>>,
    id: u32,
}
impl Monitor {
    pub fn start() -> Result<Self, Error> {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || unsafe {
            let mut message = MSG::default();
            let _ = PeekMessageW(&mut message, HWND::default(), 0, 0, PM_NOREMOVE);
            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                HMODULE::default(),
                Some(foreground_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            let destruction = SetWinEventHook(
                EVENT_OBJECT_DESTROY,
                EVENT_OBJECT_DESTROY,
                HMODULE::default(),
                Some(foreground_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            let ready = !hook.0.is_null() && !destruction.0.is_null();
            MONITOR_READY.store(ready, Ordering::Release);
            let accepted = send.send((GetCurrentThreadId(), ready)).is_ok();
            if !ready || !accepted {
                MONITOR_READY.store(false, Ordering::Release);
                if !hook.0.is_null() {
                    let _ = UnhookWinEvent(hook);
                }
                if !destruction.0.is_null() {
                    let _ = UnhookWinEvent(destruction);
                }
                return;
            }
            while GetMessageW(&mut message, HWND::default(), 0, 0).0 > 0 {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            MONITOR_READY.store(false, Ordering::Release);
            if !UnhookWinEvent(hook).as_bool() {
                eprintln!("DTW Windows foreground observer unhook failed");
            }
            if !UnhookWinEvent(destruction).as_bool() {
                eprintln!("DTW Windows destruction observer unhook failed");
            }
        });
        let (id, ready) = receive
            .recv_timeout(Duration::from_secs(2))
            .map_err(failed)?;
        if !ready {
            let _ = thread.join();
            return Err(failed("Cannot observe Windows foreground handoffs"));
        }
        Ok(Self {
            thread: Some(thread),
            id,
        })
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        if unsafe { PostThreadMessageW(self.id, WM_QUIT, WPARAM(0), LPARAM(0)) }.is_ok() {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
pub fn admit(app: &AppInstance, window: HWND) -> Result<(), Error> {
    let _settlement = SETTLEMENT.lock().unwrap();
    let mut active = ACTIVE.lock().unwrap();
    if let Some(scope) = active.as_mut() {
        if scope.app != *app {
            return Err(Error::TargetChanged);
        }
        scope.observe(unsafe { GetForegroundWindow() });
        return if scope.external {
            Err(handoff_error())
        } else {
            scope.expect_receiver(window)
        };
    }
    if !MONITOR_READY.load(Ordering::Acquire) {
        return Err(failed(
            "Windows foreground handoff observer is unavailable; no input issued",
        ));
    }
    let start_tick = unsafe { GetTickCount() };
    // A hidden foreign owner does not disable preparation of a visible
    // receiver. Returning focus still requires proof of the original owner.
    let original = Identity::original(foreground());
    if std::env::var("DTW_INPUT_TRACE").ok().as_deref() == Some("1") {
        eprintln!(
            "DTW_WINDOWS_SCOPE admit target={} original={} tick={}",
            window.0 as usize, original.hwnd, start_tick
        );
    }
    let position = cursor()?;
    let initial_windows = super::native::all_windows()?
        .into_iter()
        .map(|window| window.0 as usize)
        .collect();
    *active = Some(Scope {
        app: app.clone(),
        expected: original.clone(),
        original,
        start_tick,
        last_foreground: None,
        next: None,
        initial_windows,
        destroyed: HashSet::new(),
        parents: HashSet::new(),
        modal_returns: HashSet::new(),
        borrowed: false,
        external: false,
        cursor_original: position,
        cursor_last: position,
        pointer_borrowed: false,
        evidence_lost: false,
        last_hidden: None,
    });
    active.as_mut().unwrap().expect_receiver(window)
}
pub fn check() -> Result<(), Error> {
    let mut active = ACTIVE.lock().unwrap();
    if let Some(scope) = active.as_mut() {
        scope.evidence_lost |= !MONITOR_READY.load(Ordering::Acquire);
        if scope.evidence_lost {
            return Err(observation_error());
        }
        scope.observe(unsafe { GetForegroundWindow() });
        if scope.external {
            return Err(handoff_error());
        }
    }
    Ok(())
}
pub fn before_focus(window: HWND) -> Result<(), Error> {
    check()?;
    let mut active = ACTIVE.lock().unwrap();
    if let Some(scope) = active.as_mut() {
        scope.expect_receiver(window)?;
    }
    Ok(())
}
pub fn after_focus(window: HWND) -> Result<(), Error> {
    let mut active = ACTIVE.lock().unwrap();
    if let Some(scope) = active.as_mut() {
        scope.observe(window);
        if scope.external {
            return Err(handoff_error());
        }
    }
    Ok(())
}
pub fn before_pointer() -> Result<(), Error> {
    check()?;
    if let Some(scope) = ACTIVE.lock().unwrap().as_mut() {
        if cursor()? != scope.cursor_last {
            scope.external = true;
            return Err(handoff_error());
        }
    }
    Ok(())
}
pub fn after_pointer_move() -> Result<(), Error> {
    if let Some(scope) = ACTIVE.lock().unwrap().as_mut() {
        scope.cursor_last = cursor()?;
        scope.pointer_borrowed = true;
    }
    Ok(())
}
pub fn borrowed() -> bool {
    ACTIVE
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|scope| scope.borrowed)
}
pub fn finish() -> Result<Option<FocusReturn>, Error> {
    let _settlement = SETTLEMENT.lock().unwrap();
    let _ = check();
    let Some(mut scope) = ACTIVE.lock().unwrap().take() else {
        return Ok(None);
    };
    // Never replace the user's last choice, including an away-and-return event.
    scope.observe(unsafe { GetForegroundWindow() });
    let result = if scope.evidence_lost {
        FocusReturn::Unconfirmed
    } else if scope.external {
        FocusReturn::ChangedExternally
    } else if input::cleanup_failed() {
        FocusReturn::Unconfirmed
    } else {
        let mut clean = true;
        if scope.pointer_borrowed {
            if cursor()? != scope.cursor_last {
                return Ok(Some(FocusReturn::ChangedExternally));
            }
            super::desktop::ensure_input_desktop()?;
            unsafe { SetCursorPos(scope.cursor_original.0, scope.cursor_original.1) }
                .map_err(failed)?;
            clean &= cursor()? == scope.cursor_original;
        }
        if !scope.borrowed {
            return Ok(None);
        }
        // Return keyboard focus only. Window stacking is left to the OS.
        if !scope.original.proven {
            return Ok(Some(FocusReturn::Unconfirmed));
        }
        if scope.destroyed.contains(&scope.original.hwnd) || !scope.original.live() {
            return Ok(Some(FocusReturn::PreviousAppClosed));
        }
        let until = Instant::now() + Duration::from_millis(250);
        if input::return_focus(hwnd(scope.original.hwnd), until).is_ok()
            && scope.original.live()
            && foreground() == hwnd(scope.original.hwnd)
            && clean
        {
            FocusReturn::Restored
        } else {
            FocusReturn::Unconfirmed
        }
    };
    if std::env::var("DTW_INPUT_TRACE").ok().as_deref() == Some("1") {
        eprintln!(
            "DTW_WINDOWS_SCOPE finish focus={} pointer_borrowed={}",
            result.as_str(),
            scope.pointer_borrowed
        );
    }
    Ok(Some(result))
}

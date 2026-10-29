// Adapted from trycua/cua b38360443960a6540685a9a5eb5407f10fe3d03b.
// Source: libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs
// Copyright (c) 2025 Cua AI, Inc. MIT. See docs/background-input-port.md.

use libc::pid_t;
use std::ffi::{c_void, CStr};
use std::os::raw::{c_char, c_int, c_uint};
use std::sync::OnceLock;

type PostToPidFn = unsafe extern "C" fn(pid_t, *mut c_void);

type SetAuthMsgFn = unsafe extern "C" fn(*mut c_void, *mut c_void);

type SetWindowLocFn = unsafe extern "C" fn(*mut c_void, f64, f64);

type SetIntFieldFn = unsafe extern "C" fn(*mut c_void, u32, i64);

type ConnectionIDFn = unsafe extern "C" fn() -> u32;

type GetWindowOwnerFn = unsafe extern "C" fn(u32, u32, *mut u32) -> i32;

type GetConnectionPSNFn = unsafe extern "C" fn(u32, *mut c_void) -> i32;

type PostEventRecordToFn = unsafe extern "C" fn(*const c_void, *const u8) -> i32;

type GetFrontProcessFn = unsafe extern "C" fn(*mut c_void) -> i32;

type GetProcessForPIDFn = unsafe extern "C" fn(pid_t, *mut c_void) -> i32;

type FactoryMsgSendFn = unsafe extern "C" fn(
    *mut c_void, // Class (receiver)
    *mut c_void, // SEL
    *mut c_void, // SLSEventRecord*
    c_int,       // pid
    c_uint,      // version
) -> *mut c_void;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPrime {
    AlreadyKey,
    /// The in-app key-window records ([`post_window_key_records`]).
    KeyRecords,
    AxFocusedWindow,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyFocus {
    /// The app's `AXFocusedWindow`.
    pub window: Option<u32>,
    /// The window holding the app's `AXFocusedUIElement`.
    pub element_window: Option<u32>,
}

impl KeyFocus {
    /// Keys posted to the app go to `target`. The focused window is the
    /// predictor: in every LibreOffice trial keys went to `AXFocusedWindow`
    /// (a sibling document or Find & Replace when the switch failed, the
    /// target when it held). The focused element is not used here: VCL keeps
    /// reporting a modeless dialog's field as focused after its document
    /// window has become key and is receiving the keys.
    fn routes_to(self, target: u32) -> bool {
        self.window == Some(target)
    }

    /// `target` is key with nothing left to prime. A focused window without
    /// any focused element is a LibreOffice that has never been active: it
    /// drops every key until the window is primed (CUA-1767).
    fn settled_on(self, target: u32) -> bool {
        self.routes_to(target) && self.element_window.is_some()
    }
}
pub fn prime_exact_window_key(
    target: u32,
    mut focus: impl FnMut() -> KeyFocus,
    mut key_records: impl FnMut() -> bool,
    mut ax_focus: impl FnMut() -> bool,
) -> Result<KeyPrime, String> {
    if focus().settled_on(target) {
        return Ok(KeyPrime::AlreadyKey);
    }
    // The records go out at most twice: WindowServer delivers them
    // asynchronously, and the second pass catches an app that was still
    // handling the first.
    for _ in 0..2 {
        if key_records() && focus().routes_to(target) {
            return Ok(KeyPrime::KeyRecords);
        }
    }
    if ax_focus() && focus().routes_to(target) {
        return Ok(KeyPrime::AxFocusedWindow);
    }
    let now = focus();
    Err(format!(
        "window {target} could not be confirmed as the app's key window (the app's focused \
         window is {:?}, its focused element is in window {:?}), so no keys were sent: they \
         could have gone to another window of the same app. Refresh the app/window state",
        now.window, now.element_window
    ))
}

fn set_app_focused_window(pid: pid_t, wid: u32) -> bool {
    use super::ax::{
        ax_get_window_id, copy_ax_windows, AXUIElementCreateApplication,
        AXUIElementSetAttributeValue,
    };
    use core_foundation::{
        base::{CFRelease, TCFType},
        string::CFString,
    };
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return false;
        }
        let attr = CFString::new("AXFocusedWindow");
        let mut ok = false;
        for window in copy_ax_windows(app) {
            if !ok && ax_get_window_id(window) == Some(wid) {
                if !super::control::before_preparation() {
                    CFRelease(window as _);
                    continue;
                }
                ok = AXUIElementSetAttributeValue(
                    app,
                    attr.as_concrete_TypeRef(),
                    window as core_foundation::base::CFTypeRef,
                ) == 0;
            }
            CFRelease(window as _);
        }
        CFRelease(app as _);
        ok
    }
}

fn key_focus_of_pid(pid: pid_t) -> KeyFocus {
    KeyFocus {
        window: super::ax::focused_window_id_of_pid(pid),
        element_window: super::ax::focused_element_window_id_of_pid(pid),
    }
}

pub fn post_window_key_records(pid: pid_t, current: Option<u32>, target: u32) -> bool {
    post_window_key_records_checked(pid, current, target, None)
}

fn post_window_key_records_checked(
    pid: pid_t,
    current: Option<u32>,
    target: u32,
    evidence: Option<&super::ax::WindowEvidence>,
) -> bool {
    let Some(post) = post_event_record_to_fn() else {
        return false;
    };
    let mut psn = [0u8; 8];
    if !get_process_psn_for_window(target, pid, &mut psn) {
        return false;
    }
    let send = |record: &[u8; 0xF8]| {
        if evidence.is_some_and(|window| !window.still_matches()) {
            super::control::preparation_failure(
                "previous key window changed during restoration".into(),
            );
            return false;
        }
        if !super::control::before_preparation() {
            return false;
        }
        note_post_status(unsafe { post(psn.as_ptr() as *const c_void, record.as_ptr()) })
    };
    let mut ok = true;
    if let Some(current) = current {
        let mut defocus = focus_record(current);
        defocus[0x8A] = 0x02;
        ok &= send(&defocus);
        super::control::sleep(KEY_RECORD_SETTLE);
    }
    let mut focus = focus_record(target);
    focus[0x8A] = 0x01;
    ok &= send(&focus);
    super::control::sleep(KEY_RECORD_SETTLE);
    for event_kind in [0x01, 0x02] {
        ok &= send(&make_key_window_record(target, event_kind));
    }
    ok
}

fn await_key_focus(pid: pid_t, target: u32, budget: std::time::Duration) {
    super::control::sleep(KEY_RECORD_SETTLE);
    let deadline = std::time::Instant::now() + budget;
    while !key_focus_of_pid(pid).settled_on(target) && std::time::Instant::now() < deadline {
        if super::control::check().is_err() {
            break;
        }
        super::control::sleep(std::time::Duration::from_millis(40));
    }
}

pub(crate) fn note_post_status(status: i32) -> bool {
    if status == WINDOWSERVER_INVALID_CONNECTION {
        WINDOWSERVER_CONNECTION_INVALID.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if status != 0 {
        super::control::preparation_failure(format!("WindowServer focus record failed ({status})"));
    }
    status == 0
}

pub fn windowserver_connection_invalid() -> bool {
    WINDOWSERVER_CONNECTION_INVALID.load(std::sync::atomic::Ordering::Relaxed)
}

fn ensure_skylight_loaded() {
    static LOADED: OnceLock<()> = OnceLock::new();
    LOADED.get_or_init(|| {
        let path = b"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight\0";
        unsafe {
            libc::dlopen(
                path.as_ptr() as *const c_char,
                libc::RTLD_LAZY | libc::RTLD_GLOBAL,
            );
        }
    });
}

fn find_sym(name: &[u8]) -> Option<*mut c_void> {
    ensure_skylight_loaded();
    let ptr = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr() as *const c_char) };
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

unsafe fn as_fn<T: Copy>(ptr: *mut c_void) -> T {
    std::mem::transmute_copy::<*mut c_void, T>(&ptr)
}

fn post_to_pid_fn() -> Option<PostToPidFn> {
    static SYM: OnceLock<Option<PostToPidFn>> = OnceLock::new();
    *SYM.get_or_init(|| find_sym(b"SLEventPostToPid\0").map(|p| unsafe { as_fn(p) }))
}

fn set_auth_msg_fn() -> Option<SetAuthMsgFn> {
    static SYM: OnceLock<Option<SetAuthMsgFn>> = OnceLock::new();
    *SYM.get_or_init(|| find_sym(b"SLEventSetAuthenticationMessage\0").map(|p| unsafe { as_fn(p) }))
}

fn set_window_loc_fn() -> Option<SetWindowLocFn> {
    static SYM: OnceLock<Option<SetWindowLocFn>> = OnceLock::new();
    *SYM.get_or_init(|| find_sym(b"CGEventSetWindowLocation\0").map(|p| unsafe { as_fn(p) }))
}

fn set_int_field_fn() -> Option<SetIntFieldFn> {
    static SYM: OnceLock<Option<SetIntFieldFn>> = OnceLock::new();
    *SYM.get_or_init(|| find_sym(b"SLEventSetIntegerValueField\0").map(|p| unsafe { as_fn(p) }))
}

fn connection_id_fn() -> Option<ConnectionIDFn> {
    static SYM: OnceLock<Option<ConnectionIDFn>> = OnceLock::new();
    *SYM.get_or_init(|| find_sym(b"CGSMainConnectionID\0").map(|p| unsafe { as_fn(p) }))
}

fn factory_msg_send_fn() -> Option<FactoryMsgSendFn> {
    static SYM: OnceLock<Option<FactoryMsgSendFn>> = OnceLock::new();
    *SYM.get_or_init(|| find_sym(b"objc_msgSend\0").map(|p| unsafe { as_fn(p) }))
}

fn get_window_owner_fn() -> Option<GetWindowOwnerFn> {
    static SYM: OnceLock<Option<GetWindowOwnerFn>> = OnceLock::new();
    *SYM.get_or_init(|| find_sym(b"SLSGetWindowOwner\0").map(|p| unsafe { as_fn(p) }))
}

fn get_connection_psn_fn() -> Option<GetConnectionPSNFn> {
    static SYM: OnceLock<Option<GetConnectionPSNFn>> = OnceLock::new();
    *SYM.get_or_init(|| find_sym(b"SLSGetConnectionPSN\0").map(|p| unsafe { as_fn(p) }))
}

fn post_event_record_to_fn() -> Option<PostEventRecordToFn> {
    static SYM: OnceLock<Option<PostEventRecordToFn>> = OnceLock::new();
    *SYM.get_or_init(|| find_sym(b"SLPSPostEventRecordTo\0").map(|p| unsafe { as_fn(p) }))
}

fn get_front_process_fn() -> Option<GetFrontProcessFn> {
    static SYM: OnceLock<Option<GetFrontProcessFn>> = OnceLock::new();
    *SYM.get_or_init(|| find_sym(b"_SLPSGetFrontProcess\0").map(|p| unsafe { as_fn(p) }))
}

fn get_process_for_pid_fn() -> Option<GetProcessForPIDFn> {
    static SYM: OnceLock<Option<GetProcessForPIDFn>> = OnceLock::new();
    *SYM.get_or_init(|| find_sym(b"GetProcessForPID\0").map(|p| unsafe { as_fn(p) }))
}

pub fn is_available() -> bool {
    post_to_pid_fn().is_some()
}

pub fn is_focus_without_raise_available() -> bool {
    let has_psn_lookup = (connection_id_fn().is_some()
        && get_window_owner_fn().is_some()
        && get_connection_psn_fn().is_some())
        || get_process_for_pid_fn().is_some();
    get_front_process_fn().is_some() && has_psn_lookup && post_event_record_to_fn().is_some()
}

fn objc_class(name: &CStr) -> *mut c_void {
    type GetClassFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    static SYM: OnceLock<Option<GetClassFn>> = OnceLock::new();
    let f = *SYM.get_or_init(|| find_sym(b"objc_getClass\0").map(|p| unsafe { as_fn(p) }));
    match f {
        Some(f) => unsafe { f(name.as_ptr()) },
        None => std::ptr::null_mut(),
    }
}

fn sel_register(name: &CStr) -> *mut c_void {
    type SelRegFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    static SYM: OnceLock<Option<SelRegFn>> = OnceLock::new();
    let f = *SYM.get_or_init(|| find_sym(b"sel_registerName\0").map(|p| unsafe { as_fn(p) }));
    match f {
        Some(f) => unsafe { f(name.as_ptr()) },
        None => std::ptr::null_mut(),
    }
}

fn class_responds_to_selector(cls: *mut c_void, sel: *mut c_void) -> bool {
    if cls.is_null() || sel.is_null() {
        return false;
    }
    type RespondsToFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool;
    static SYM: OnceLock<Option<RespondsToFn>> = OnceLock::new();
    let f =
        *SYM.get_or_init(|| find_sym(b"class_respondsToSelector\0").map(|p| unsafe { as_fn(p) }));
    match f {
        Some(f) => {
            // Factory is a class method. CUA queried the instance method table.
            type ObjectClassFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
            let Some(get_class) =
                find_sym(b"object_getClass\0").map(|p| unsafe { as_fn::<ObjectClassFn>(p) })
            else {
                return false;
            };
            let metaclass = unsafe { get_class(cls) };
            !metaclass.is_null() && unsafe { f(metaclass, sel) }
        }
        None => false,
    }
}

unsafe fn extract_event_record(event_ptr: *mut c_void) -> *mut c_void {
    for &offset in &[24usize, 32, 16] {
        let slot = (event_ptr as *const u8).add(offset).cast::<*mut c_void>();
        let p = std::ptr::read_unaligned(slot);
        if !p.is_null() {
            return p;
        }
    }
    std::ptr::null_mut()
}

pub(super) fn post_to_pid(pid: pid_t, event_ptr: *mut c_void, attach_auth_message: bool) -> bool {
    let post_fn = match post_to_pid_fn() {
        Some(f) => f,
        None => return false,
    };

    if attach_auth_message {
        let mut authenticated = false;
        // Build and attach SLSEventAuthenticationMessage.
        //
        // macOS 14 (Sonoma) compatibility: the class exists on macOS 14 but
        // `messageWithEventRecord:pid:version:` was added in macOS 15. Guard
        // with `class_respondsToSelector` (a `!sel.is_null()` check is not
        // enough — `sel_registerName` interns any name); when the selector is
        // absent, DTW declines background keyboard before preparing a window.
        // If authentication construction fails after preparation, stop without
        // posting an unauthenticated event or replaying through another route.
        let cls = objc_class(c"SLSEventAuthenticationMessage");
        let sel = sel_register(c"messageWithEventRecord:pid:version:");
        let factory = factory_msg_send_fn();

        if class_responds_to_selector(cls, sel) {
            if let Some(factory_fn) = factory {
                let record = unsafe { extract_event_record(event_ptr) };
                if !record.is_null() {
                    let msg = unsafe { factory_fn(cls, sel, record, pid as c_int, 0u32) };
                    if !msg.is_null() {
                        if let Some(set_auth) = set_auth_msg_fn() {
                            unsafe { set_auth(event_ptr, msg) };
                            authenticated = true;
                        }
                    }
                }
            }
        }
        if !authenticated {
            return false;
        }
    }

    unsafe { post_fn(pid, event_ptr) };
    true
}

pub(super) fn set_window_location(event_ptr: *mut c_void, x: f64, y: f64) -> bool {
    match set_window_loc_fn() {
        Some(f) => {
            unsafe { f(event_ptr, x, y) };
            true
        }
        None => false,
    }
}

pub(super) fn set_integer_field(event_ptr: *mut c_void, field: u32, value: i64) -> bool {
    match set_int_field_fn() {
        Some(f) => {
            unsafe { f(event_ptr, field, value) };
            true
        }
        None => false,
    }
}

pub fn activate_without_raise_with(
    target_pid: pid_t,
    target_wid: u32,
    defocus_front: bool,
) -> bool {
    let post_fn = match post_event_record_to_fn() {
        Some(f) => f,
        None => return false,
    };
    let get_front = match get_front_process_fn() {
        Some(f) => f,
        None => return false,
    };
    // 8-byte PSN buffers (two UInt32s).
    let mut prev_psn = [0u8; 8];
    let mut target_psn = [0u8; 8];

    let ok_prev = unsafe { get_front(prev_psn.as_mut_ptr() as *mut c_void) } == 0;
    if !ok_prev {
        return false;
    }

    if !get_process_psn_for_window(target_wid, target_pid, &mut target_psn) {
        return false;
    }

    // Build the 248-byte event buffer.
    let mut buf = [0u8; 0xF8];
    buf[0x04] = 0xF8;
    buf[0x08] = 0x0D;
    // Stamp target window id in little-endian at bytes 0x3c–0x3f.
    buf[0x3C] = (target_wid & 0xFF) as u8;
    buf[0x3D] = ((target_wid >> 8) & 0xFF) as u8;
    buf[0x3E] = ((target_wid >> 16) & 0xFF) as u8;
    buf[0x3F] = ((target_wid >> 24) & 0xFF) as u8;

    // Step 3: defocus previous front.
    buf[0x8A] = 0x02;
    if !super::control::before_preparation() {
        return false;
    }
    let defocus_ok = !defocus_front
        || note_post_status(unsafe { post_fn(prev_psn.as_ptr() as *const c_void, buf.as_ptr()) });

    // Step 4: focus target.
    buf[0x8A] = 0x01;
    let focus_ok =
        note_post_status(unsafe { post_fn(target_psn.as_ptr() as *const c_void, buf.as_ptr()) });

    defocus_ok && focus_ok
}

fn focus_record(wid: u32) -> [u8; 0xF8] {
    let mut buf = [0u8; 0xF8];
    buf[0x04] = 0xF8;
    buf[0x08] = 0x0D;
    buf[0x3C..0x40].copy_from_slice(&wid.to_le_bytes());
    buf
}

pub fn get_process_psn_for_window(window_id: u32, pid: libc::pid_t, out_psn: &mut [u8; 8]) -> bool {
    // Try modern path: CGSMainConnectionID → SLSGetWindowOwner → SLSGetConnectionPSN
    if let (Some(get_owner), Some(get_psn), Some(conn_id_fn)) = (
        get_window_owner_fn(),
        get_connection_psn_fn(),
        connection_id_fn(),
    ) {
        let main_cid = unsafe { conn_id_fn() };
        let mut owner_cid: u32 = 0;
        let owner_status = unsafe { get_owner(main_cid, window_id, &mut owner_cid) };
        // DTW preflight must not hide an invalid caller connection behind
        // GetProcessForPID: only a read has occurred, so foreground is still safe.
        if owner_status == WINDOWSERVER_INVALID_CONNECTION {
            WINDOWSERVER_CONNECTION_INVALID.store(true, std::sync::atomic::Ordering::Relaxed);
            return false;
        }
        if owner_status == 0 && owner_cid != 0 {
            let psn_ok = unsafe { get_psn(owner_cid, out_psn.as_mut_ptr() as *mut c_void) } == 0;
            if psn_ok {
                return true;
            }
        }
    }
    // Fallback: GetProcessForPID
    if let Some(get_pid_psn) = get_process_for_pid_fn() {
        return unsafe { get_pid_psn(pid, out_psn.as_mut_ptr() as *mut c_void) } == 0;
    }
    false
}

fn make_key_window_record(window_id: u32, event_kind: u8) -> [u8; 0xF8] {
    let mut record = [0u8; 0xF8];
    record[0x04] = 0xF8;
    record[0x08] = event_kind;
    record[0x3A] = 0x10;
    record[0x3C..0x40].copy_from_slice(&window_id.to_le_bytes());
    record[0x20..0x30].fill(0xFF);
    record
}

const KEY_RECORD_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);
static WINDOWSERVER_CONNECTION_INVALID: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub fn pointer_available() -> bool {
    is_available()
        && is_focus_without_raise_available()
        && set_window_loc_fn().is_some()
        && set_int_field_fn().is_some()
        && !windowserver_connection_invalid()
}
pub fn keyboard_available() -> bool {
    pointer_available()
        && set_auth_msg_fn().is_some()
        && factory_msg_send_fn().is_some()
        && class_responds_to_selector(
            objc_class(c"SLSEventAuthenticationMessage"),
            sel_register(c"messageWithEventRecord:pid:version:"),
        )
}

const WINDOWSERVER_INVALID_CONNECTION: i32 = 1002;

fn should_restore_key_window(current: Option<u32>, previous: Option<u32>, target: u32) -> bool {
    current == Some(target) && current != previous
}

/// Port of CUA's exact-window key scope with explicit, identity-checked cleanup.
/// It restores only the retained previous window, never a newly discovered sibling.
pub struct WindowKeyScope {
    pid: pid_t,
    target: u32,
    previous: Option<u32>,
    previous_element: Option<super::ax::WindowEvidence>,
    finished: bool,
}
impl WindowKeyScope {
    pub fn capture(pid: pid_t, target: u32) -> Self {
        let previous = key_focus_of_pid(pid).window;
        let previous_element = previous.and_then(|id| super::ax::WindowEvidence::retain(pid, id));
        Self {
            pid,
            target,
            previous,
            previous_element,
            finished: false,
        }
    }
    pub fn can_restore(&self) -> bool {
        self.previous.is_some()
            && self
                .previous_element
                .as_ref()
                .is_some_and(|w| w.still_matches())
    }
    pub fn prime(&mut self) -> anyhow::Result<()> {
        prime_exact_window_key(
            self.target,
            || key_focus_of_pid(self.pid),
            || {
                let ok = post_window_key_records(self.pid, self.previous, self.target);
                if ok {
                    await_key_focus(self.pid, self.target, std::time::Duration::from_millis(400));
                }
                ok
            },
            || {
                let ok = set_app_focused_window(self.pid, self.target);
                if ok {
                    await_key_focus(self.pid, self.target, std::time::Duration::from_millis(200));
                }
                ok
            },
        )
        .map(|_| ())
        .map_err(anyhow::Error::msg)
    }
    fn restore(&self) -> anyhow::Result<()> {
        if !super::control::prepared() {
            return Ok(());
        }
        // The target can legitimately close or lose key ownership after input.
        // Verify the original app first, and preserve any subsequent focus choice.
        super::control::check_input_owner()?;
        let current = key_focus_of_pid(self.pid).window;
        if !should_restore_key_window(current, self.previous, self.target) {
            return Ok(());
        }
        // Only an owned restoration may touch windows: retain the full original
        // target check and the original previous AX object, never a new sibling.
        super::control::check()?;
        let previous = self.previous.ok_or_else(|| {
            anyhow::anyhow!("previous key window is unknown; focus cannot be restored safely")
        })?;
        anyhow::ensure!(
            self.previous_element
                .as_ref()
                .is_some_and(|w| w.still_matches()),
            "previous key window is no longer valid"
        );
        anyhow::ensure!(
            post_window_key_records_checked(
                self.pid,
                Some(self.target),
                previous,
                self.previous_element.as_ref()
            ),
            "restoring previous key window failed"
        );
        await_key_focus(self.pid, previous, std::time::Duration::from_millis(300));
        anyhow::ensure!(
            key_focus_of_pid(self.pid).window == Some(previous),
            "previous key window restoration could not be verified"
        );
        Ok(())
    }
    pub fn cleanup(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        super::control::cleanup_pending();
        if !super::control::prepared() {
            return;
        }
        super::control::with_cleanup(|| {
            if let Err(error) = self.restore() {
                super::control::cleanup_failure(error.to_string());
            }
        });
    }
}
impl Drop for WindowKeyScope {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lost_key_ownership_preserves_current_focus() {
        assert!(should_restore_key_window(Some(7), Some(6), 7));
        assert!(!should_restore_key_window(Some(6), Some(6), 7));
        assert!(!should_restore_key_window(Some(8), Some(6), 7));
        assert!(!should_restore_key_window(None, Some(6), 7));
        assert!(!should_restore_key_window(Some(7), Some(7), 7));
    }
    #[test]
    fn exact_window_does_not_reprime() {
        let result = prime_exact_window_key(
            9,
            || KeyFocus {
                window: Some(9),
                element_window: Some(9),
            },
            || panic!("already key"),
            || panic!("already key"),
        );
        assert_eq!(result, Ok(KeyPrime::AlreadyKey));
    }
    #[test]
    fn sibling_window_is_not_accepted() {
        let result = prime_exact_window_key(
            9,
            || KeyFocus {
                window: Some(8),
                element_window: Some(8),
            },
            || true,
            || true,
        );
        assert!(result.is_err());
    }
    #[test]
    fn records_must_be_followed_by_exact_window_proof() {
        use std::cell::Cell;
        let window = Cell::new(8);
        let result = prime_exact_window_key(
            9,
            || KeyFocus {
                window: Some(window.get()),
                element_window: Some(window.get()),
            },
            || {
                window.set(9);
                true
            },
            || false,
        );
        assert_eq!(result, Ok(KeyPrime::KeyRecords));
    }
}

pub(super) fn key_window_matches(pid: pid_t, target: u32) -> bool {
    super::ax::focused_window_id_of_pid(pid) == Some(target)
}

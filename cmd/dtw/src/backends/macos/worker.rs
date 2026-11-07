//! Private worker entry; sequential native ownership and a parent-death watchdog.
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    ffi::{c_void, CStr, CString},
    io::{BufRead, Read, Write},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[path = "../../scene_lease.rs"]
pub(super) mod scene_lease;
unsafe extern "C" {
    fn dtw_native_call(request: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    fn dtw_native_free(reply: *mut std::ffi::c_char);
    fn dtw_set_pointer_presenter(callback: Option<extern "C" fn(f64, f64, i32)>);
    fn dtw_set_associated_surface_prover(
        callback: Option<extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_char>,
        free: Option<extern "C" fn(*mut std::ffi::c_char)>,
    );
    fn dtw_scene_call(request: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    fn dtw_scene_free(reply: *mut std::ffi::c_char);
    fn dtw_input_quiesce(timeout_milliseconds: u32) -> bool;
    fn dtw_focus_quiesce(timeout_milliseconds: u32) -> bool;
    fn dtw_focus_owner(timeout_milliseconds: u32, pid: *mut u32, generation: *mut u64) -> i32;
}

static TERMINATING: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_STARTED: AtomicBool = AtomicBool::new(false);

pub(super) fn stopping() -> bool {
    TERMINATING.load(Ordering::Relaxed)
}

extern "C" fn request_termination(_: libc::c_int) {
    // A signal handler only touches a lock-free atomic. No Swift, locks, IO,
    // allocation, or AppKit calls are permitted here.
    TERMINATING.store(true, Ordering::Relaxed);
}

fn install_termination_handler() -> std::io::Result<()> {
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = request_termination as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        action.sa_flags = libc::SA_RESTART;
        if libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

fn finish_focus_shutdown(until: Instant) -> bool {
    let left = || until.saturating_duration_since(Instant::now());
    let (mut pid, mut generation) = (0, 0);
    match unsafe { dtw_focus_owner(left().as_millis() as u32, &mut pid, &mut generation) } {
        0 => unsafe { dtw_focus_quiesce(left().as_millis() as u32) },
        1 => {
            // Return can modify the previous app as well as the borrowed app.
            // An in-flight action (including ours) may own a shared lease; wait
            // before acquiring any instance/physical lock, never restore unlocked.
            let Ok(_coordination) =
                scene_lease::lock_coordination(scene_lease::InputCoordination::Foreground, left())
            else {
                return false;
            };
            let Ok(_instance) = scene_lease::lock_instance(pid, generation, left()) else {
                return false;
            };
            let Ok(_physical) = scene_lease::lock_input(left()) else {
                return false;
            };
            unsafe { dtw_focus_quiesce(left().as_millis() as u32) }
        }
        _ => false,
    }
}
fn shutdown_worker() -> ! {
    TERMINATING.store(true, Ordering::Relaxed);
    if SHUTDOWN_STARTED.swap(true, Ordering::AcqRel) {
        // One owner drains input and returns focus. AppKit activation on the
        // watchdog can wait for the main run loop; parking that thread deadlocks
        // restoration. Dispatch has returned and released its action leases here.
        // Service native callbacks only, without admitting requests or taking
        // cleanup ownership. The existing owner/parent deadlines still apply.
        let main_thread = unsafe { libc::pthread_main_np() } != 0;
        loop {
            if main_thread {
                core_foundation::runloop::CFRunLoop::run_in_mode(
                    unsafe { core_foundation::runloop::kCFRunLoopDefaultMode },
                    Duration::from_millis(10),
                    false,
                );
                // An empty or stopped run loop must not busy-spin.
                std::thread::park_timeout(Duration::from_millis(1));
            } else {
                std::thread::park_timeout(Duration::from_millis(10));
            }
        }
    }
    let until = Instant::now() + Duration::from_millis(400);
    let remaining = || until.saturating_duration_since(Instant::now()).as_millis() as u32;
    let background = super::cua_input::quiesce(remaining());
    let input = unsafe { dtw_input_quiesce(remaining()) };
    let focus = finish_focus_shutdown(until);
    if !(background && input && focus) {
        eprintln!("dtw native input cleanup did not complete before shutdown");
    }
    std::process::exit(if background && input && focus {
        0
    } else {
        super::super::process::INPUT_QUIESCE_FAILED_EXIT_CODE
    });
}

pub(super) fn native(request: &Value, scene: bool) -> Value {
    let input = CString::new(request.to_string()).expect("JSON has no literal NUL");
    unsafe {
        let ptr = if scene {
            dtw_scene_call(input.as_ptr())
        } else {
            dtw_native_call(input.as_ptr())
        };
        if ptr.is_null() {
            return json!({"error":"invalid_response","message":"Empty native response","dispatch":"unknown","evidence":"unverified"});
        }
        let result = serde_json::from_slice(CStr::from_ptr(ptr).to_bytes()).unwrap_or_else(
            |_| json!({"error":"invalid_response","dispatch":"unknown","evidence":"unverified"}),
        );
        if scene {
            dtw_scene_free(ptr);
        } else {
            dtw_native_free(ptr);
        }
        result
    }
}
fn input_operation(op: &str) -> bool {
    matches!(
        op,
        "press"
            | "set_value"
            | "focus"
            | "type_text"
            | "keypress"
            | "keypress_app"
            | "focus_app"
            | "scroll"
            | "click"
            | "move"
            | "drag"
            | "quit_app"
            | "close_window"
            | "begin_execution_focus"
    )
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn trace_now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1_000.0
}
fn route_trace(request: &Value, route: &str, reason: &str) -> Value {
    let mut record = json!({"at_unix_ms":trace_now_ms(),"pid":std::process::id(),
        "operation":request["operation"],"window":request["target"]["native_id"],"route":route,"reason":reason});
    if reason == "native_keyboard_semantics" {
        record["capability_detail"] = request["_keyboard_capability"].clone();
    }
    record
}
fn trace_route(request: &Value, route: &str, reason: &str) {
    if std::env::var("DTW_INPUT_TRACE").ok().as_deref() == Some("1") {
        emit_input_trace("dtw_input", &route_trace(request, route, reason));
    }
}
fn emit_input_trace(kind: &str, record: &Value) {
    let line = format!("{kind} {record}\n");
    eprint!("{line}");
    // Some hosts discard child stderr on failed tools. An opt-in private file
    // keeps the same bounded, payload-free records independently of the host.
    if let Some(path) = std::env::var_os("DTW_INPUT_TRACE_FILE") {
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            if file.metadata().is_ok_and(|metadata| metadata.is_file()) {
                let _ = file.write_all(line.as_bytes());
            }
        }
    }
}
pub(super) fn trace_execution_refusal(request: &Value) {
    if std::env::var("DTW_INPUT_TRACE").ok().as_deref() == Some("1") {
        let mut record = route_trace(request, "refused", "execution_scope_preflight");
        record["dispatch"] = json!("not_dispatched");
        emit_input_trace("dtw_input", &record);
    }
}
// Owned metadata survives dispatch_owned consuming the request. Drop closes even
// early-error paths; a killed worker leaves an incomplete interval for the audit.
struct PointerBorrowTrace(Value);
impl PointerBorrowTrace {
    fn begin(request: &Value, reason: &str) -> Option<Self> {
        if std::env::var("DTW_INPUT_TRACE").ok().as_deref() != Some("1") {
            return None;
        }
        let trace = Self(Self::metadata(request, reason));
        trace.emit("begin");
        Some(trace)
    }
    fn metadata(request: &Value, reason: &str) -> Value {
        static NEXT_TRACE: AtomicU64 = AtomicU64::new(1);
        json!({"pid":std::process::id(),
            "trace_id":NEXT_TRACE.fetch_add(1, Ordering::Relaxed),
            "operation":request["operation"],"window":request["target"]["native_id"],
            "route":"borrowed","reason":reason})
    }
    fn emit(&self, phase: &str) {
        let mut record = self.0.clone();
        record["phase"] = json!(phase);
        record["at_unix_ms"] = json!(trace_now_ms());
        emit_input_trace("dtw_input_interval", &record);
    }
}
impl Drop for PointerBorrowTrace {
    fn drop(&mut self) {
        self.emit("end");
    }
}

pub(super) fn remaining(request: &Value) -> Duration {
    Duration::from_millis(
        request["deadline_unix_ms"]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(now_ms()),
    )
}

struct BackgroundTarget {
    target: Value,
    surface_owner: Value,
    bounds: Value,
    keyboard: bool,
    native_modal: bool,
    foreground: bool,
    deadline: u64,
    failures: RefCell<BackgroundValidationFailures>,
}
#[derive(Default)]
struct BackgroundValidationFailures {
    action: Option<Value>,
    cleanup: Option<Value>,
}
impl BackgroundValidationFailures {
    fn record(&mut self, phase: i32, error: Value) {
        if phase == 0 {
            &mut self.action
        } else {
            &mut self.cleanup
        }
        .get_or_insert(error);
    }
    fn apply(&self, result: &mut Value) {
        // The C callback's integer result controls admission, but cannot carry
        // a native diagnosis. Keep the first action and cleanup causes distinct;
        // later cleanup must never replace the original uncertain action result.
        for (cause, field, detail) in [
            (&self.action, "error", "validation_error"),
            (&self.cleanup, "cleanup_error", "cleanup_validation_error"),
        ] {
            if let Some(cause) = cause {
                if result[field].is_object() {
                    // Focus loss has a stable public stop condition even when
                    // the addressed-event callback carries it through a C ABI.
                    if cause["error"] == "focus_changed" {
                        result[field]["code"] = cause["error"].clone();
                    }
                    result[field]["message"] = json!(format!(
                        "{}: {}",
                        cause["error"].as_str().unwrap_or("validation_failed"),
                        cause["message"]
                            .as_str()
                            .unwrap_or("Target validation unavailable")
                    ));
                    result[detail] = cause.clone();
                }
            }
        }
    }
}
extern "C" fn validate_background(context: *mut c_void, phase: i32) -> i32 {
    let target = unsafe { &*(context.cast::<BackgroundTarget>()) };
    if phase == 0 && (TERMINATING.load(Ordering::Relaxed) || now_ms() >= target.deadline) {
        return -1;
    }
    if phase == 2 {
        // Only releases paired with an admitted down use this phase. A closing
        // window must not strand input in its app; a reused PID is never accepted.
        let reply = native(
            &json!({"operation":"validate_input_owner", "app":target.target["app"],
            "surface_owner":target.surface_owner,
            "deadline_unix_ms":now_ms()+150}),
            false,
        );
        if reply.get("error").is_none() && reply["app"] == target.target["app"] {
            return 1;
        }
        let error = if reply["error"].is_string() {
            reply
        } else {
            json!({"error":"app_changed","message":"Original input owner could not be proven"})
        };
        target.failures.borrow_mut().record(phase, error);
        return 0;
    }
    if !matches!(phase, 0 | 1) {
        return -1;
    }
    // Window-key restoration retains the original window proof.
    // Cleanup ignores normal cancellation, focus and geometry changes.
    let check = json!({"operation":if phase == 0 {"validate_background"} else {"validate"},
        "target":target.target,"keyboard":target.keyboard,"step":phase == 0,
        "_execution_foreground":phase == 0 && target.foreground,
        "deadline_unix_ms":if phase == 0 {target.deadline} else {now_ms()+150}});
    let reply = native(&check, false);
    match background_validation(&reply, target, phase) {
        Ok(()) => {
            if phase == 0 && !target.keyboard {
                let Some((x, y)) = super::cua_input::pointer_point() else {
                    return -1;
                };
                let point = native(
                    &json!({"operation":"validate_directed_point","target":target.target,
                    "pid":target.surface_owner["pid"],"surface_owner":target.surface_owner,
                    "window_id":target.target["native_id"],"bounds":target.bounds,
                    "screen_x":x,"screen_y":y,"deadline_unix_ms":target.deadline}),
                    true,
                );
                if point["valid"] != true {
                    target.failures.borrow_mut().record(phase, point);
                    return 0;
                }
            }
            1
        }
        Err(error) => {
            target.failures.borrow_mut().record(phase, error);
            0
        }
    }
}
fn background_validation(
    reply: &Value,
    target: &BackgroundTarget,
    phase: i32,
) -> Result<(), Value> {
    validate_reply(reply, &target.target)?;
    if reply["surface_owner"] != target.surface_owner {
        return Err(
            json!({"error":"target_changed","message":"Addressed surface owner incarnation changed"}),
        );
    }
    if phase == 0 && target.keyboard && native_modal_input(reply)? != target.native_modal {
        // The initial side-effect-free preflight has already selected a route.
        // A later modal transition stops this sequence; it cannot replay input
        // through the foreground route. Paired cleanup uses phases 1/2.
        return Err(json!({"error":"background_route_changed",
            "message":"The modal keyboard context changed during background input; inspect before new work"}));
    }
    if phase == 0 && !target.keyboard && reply["bounds"] != target.bounds {
        return Err(
            json!({"error":"stale_observation","message":"Input window geometry changed; capture again before pointer input"}),
        );
    }
    Ok(())
}

enum BackgroundAttempt {
    NotApplicable,
    Declined(String),
    Completed(Value),
}

fn delivery(
    route: Option<&str>,
    reason: Option<&str>,
    borrowed: bool,
    returned: Option<&str>,
) -> Value {
    let mut value = json!({"route":route,"foreground_borrowed":borrowed});
    if let Some(reason) = reason {
        value["reason"] = json!(reason);
    }
    if let Some(returned) = returned {
        value["focus_return"] = json!(returned);
    }
    value
}

fn foreground_delivery(
    result: &Value,
    operation: &str,
    reason: Option<&str>,
    prepared_borrowed: bool,
) -> Value {
    // Native focus may renew its scope after preparation. Report its actual
    // return obligation, not just the earlier preparation snapshot.
    let borrowed = result["borrowed"].as_bool().unwrap_or(prepared_borrowed);
    delivery(
        Some("foreground"),
        reason,
        borrowed,
        (operation == "focus").then_some(if borrowed { "pending" } else { "unchanged" }),
    )
}

fn record_foreground_cleanup(result: &mut Value, cleanup: Value) {
    if let Some(point) = cleanup.get("execution_pointer") {
        result["execution_pointer"] = point.clone();
    }
    result["delivery"]["focus_return"] = if cleanup.get("error").is_some() {
        json!("unconfirmed")
    } else {
        cleanup["focus"].clone()
    };
    if cleanup.get("error").is_some() {
        // Returning focus is separate from the already-produced input result.
        // The provider's cleanup channel closes execution without erasing a
        // known dispatch or replacing the original action failure.
        result["cleanup_error"] = cleanup;
    }
}

#[cfg(test)]
#[test]
fn pointer_handoff_preserves_known_dispatch_and_primary_failure() {
    let mut result = json!({"dispatch":"dispatched", "error":"original", "delivery":{}});
    record_foreground_cleanup(
        &mut result,
        json!({"focus":"unchanged", "execution_pointer":[-800.25,125.5]}),
    );
    assert_eq!(result["dispatch"], "dispatched");
    assert_eq!(result["error"], "original");
    assert_eq!(result["execution_pointer"], json!([-800.25, 125.5]));
}

fn click_can_follow_focus(result: &Value) -> bool {
    result["dispatch"] == "dispatched" && result.get("error").is_none()
}

/// A decline is an explicit, side-effect-free preflight result. An unknown or
/// dispatched result never selects another route, even with a malformed flag.
fn background_result(mut result: Value) -> BackgroundAttempt {
    if let Some(reason) = super::input_route::fallback_reason(&result) {
        return BackgroundAttempt::Declined(reason.code().into());
    }
    if result["error"].is_object() {
        result["message"] = result["error"]["message"].clone();
        result["error"] = result["error"]["code"].clone();
    }
    result["evidence"] = json!("unverified");
    let menu = result["transport"] == "ax_menu_equivalent";
    result["route"] = json!(if menu {
        "cua_ax_menu"
    } else {
        "cua_pid_window"
    });
    result["delivery"] = delivery(
        Some("background"),
        Some(if menu {
            "native_menu_equivalent"
        } else {
            "background_supported"
        }),
        false,
        None,
    );
    BackgroundAttempt::Completed(result)
}

fn native_modal_input(validation: &Value) -> Result<bool, Value> {
    validation["background_requires_foreground"]
        .as_bool()
        .ok_or_else(
            || json!({"error":"invalid_response","message":"Missing native modal input preflight"}),
        )
        .map(|modal| modal || validation["native_open_panel"] == true)
}

fn native_menu_click(
    request: &Value,
    x: f64,
    y: f64,
    send: impl FnOnce(&Value, bool) -> Value,
) -> Value {
    let mut menu = request.clone();
    menu["operation"] = json!("try_native_menu_click");
    menu["screen_x"] = json!(x);
    menu["screen_y"] = json!(y);
    // NativeStore owns AX actions and this process's bound target. The scene
    // extension is a separate entry point for SCK and pointer presentation.
    send(&menu, false)
}

fn background(request: &mut Value, leased: &mut bool, scene_worker: bool) -> BackgroundAttempt {
    let op = request["operation"].as_str().unwrap_or("").to_owned();
    if !matches!(
        op.as_str(),
        "click" | "move" | "drag" | "scroll" | "type_text" | "keypress"
    ) {
        return BackgroundAttempt::NotApplicable;
    }
    let keyboard = matches!(op.as_str(), "type_text" | "keypress");
    // Once a script owns foreground, keep its keyboard on the existing native
    // foreground route. PID events can still be ignored by remote/modal views
    // even while their app is active. This grants no activation or reacquisition.
    if super::input_route::owned_foreground_keyboard(&op, request["_execution_foreground"] == true)
    {
        return BackgroundAttempt::Declined("owned_foreground_keyboard".into());
    }
    let check = json!({"operation":"validate_background","target":request["target"],
        "keyboard":keyboard,"consume":true,"_execution_foreground":request["_execution_foreground"] == true,
        "pointer_operation":op,"pointer_button":request["button"],
        "pointer_point":if op == "drag" { json!([request["path"][0]["x"],request["path"][0]["y"]]) }
            else { json!([request["screen_x"],request["screen_y"]]) },
        "deadline_unix_ms":request["deadline_unix_ms"]});
    let before = native(&check, false);
    if let Err(error) = validate_reply(&before, &request["target"]) {
        return BackgroundAttempt::Completed(rejected(error, &op));
    }
    request["surface_owner"] = before["surface_owner"].clone();
    if request.get("bounds").is_some() && request["bounds"] != before["bounds"] {
        return BackgroundAttempt::Completed(rejected(
            json!({"error":"stale_observation","message":"Window geometry changed; capture again before input"}),
            &op,
        ));
    }
    let bounds = before["bounds"].as_array().unwrap();
    let left = bounds[0].as_f64().unwrap();
    let top = bounds[1].as_f64().unwrap();
    let width = bounds[2].as_f64().unwrap();
    let height = bounds[3].as_f64().unwrap();
    let x = request["screen_x"].as_f64().unwrap_or(left + width / 2.0);
    let y = request["screen_y"].as_f64().unwrap_or(top + height / 2.0);
    if !x.is_finite()
        || !y.is_finite()
        || x < left
        || y < top
        || x >= left + width
        || y >= top + height
    {
        return BackgroundAttempt::Completed(rejected(
            json!({"error":"invalid_point","message":"Input point must be inside the current target window"}),
            &op,
        ));
    }
    if !keyboard {
        if op == "click"
            && before["input_framework"] == "appkit"
            && request["button"].as_str().unwrap_or("left") == "left"
            && request["count"].as_u64().unwrap_or(1) == 1
        {
            if !scene_worker {
                return BackgroundAttempt::Completed(rejected(
                    json!({"error":"invalid_request","message":"Pointer input needs its scene owner"}),
                    &op,
                ));
            }
            if let Err(error) = admit_scene(request, leased) {
                return BackgroundAttempt::Completed(rejected(error, &op));
            }
            let mut selected = native_menu_click(request, x, y, native);
            if selected["native_menu"] != false {
                trace_route(request, "native", "menu_point_press");
                selected["delivery"] =
                    delivery(Some("semantic"), Some("native_menu_point"), false, None);
                return BackgroundAttempt::Completed(selected);
            }
        }
        if op == "drag" && before["surface_owner"]["pid"] == request["target"]["app"]["pid"] {
            let mut movement = request.clone();
            movement["operation"] = json!("try_native_window_move");
            movement["pid"] = before["surface_owner"]["pid"].clone();
            movement["window_id"] = request["target"]["native_id"].clone();
            let mut moved = native(&movement, true);
            if moved["native_move"] != false {
                trace_route(request, "native", "window_position");
                moved["delivery"] = delivery(
                    Some("semantic"),
                    Some("native_window_position"),
                    false,
                    None,
                );
                return BackgroundAttempt::Completed(moved);
            }
        }
        request["screen_x"] = json!(x);
        request["screen_y"] = json!(y);
        request["_pointer_bounds"] = before["bounds"].clone();
    }
    if keyboard && before["surface_owner"]["pid"] != request["target"]["app"]["pid"] {
        // Preserve the independently proven owner for foreground capture and
        // pointer presentation, without changing the logical app identity.
        // CUA cannot direct input to a different process on this app's behalf.
        return BackgroundAttempt::Declined("associated_process_keyboard".into());
    }
    let native_modal = if keyboard {
        match native_modal_input(&before) {
            Ok(modal) => modal,
            Err(error) => return BackgroundAttempt::Completed(rejected(error, &op)),
        }
    } else {
        false
    };
    // NSRemoteView file panels consume session keyboard events, not events
    // addressed to their AppKit host. The real ESC regression proves that even
    // an already-frontmost host can ACK delivery without closing the panel.
    // Decline before CUA primes a key window or posts a payload; ordinary native
    // dialogs and text fields retain their addressed keyboard opportunities.
    if keyboard && before["native_open_panel"] == true {
        return BackgroundAttempt::Declined("native_modal_keyboard".into());
    }
    if !keyboard {
        if op == "click"
            && before["native_open_panel"] == true
            && request["button"].as_str().unwrap_or("left") == "left"
            && request["count"].as_u64().unwrap_or(1) == 1
        {
            return BackgroundAttempt::Declined("native_open_panel_click".into());
        }
        if before["pointer_key_window_required"] == true && request["_execution_foreground"] != true
        {
            // Clean preflight only. Once the script owns its key window, retry
            // the original CUA addressed payload without a real-pointer warp.
            return BackgroundAttempt::Declined("native_pointer_key_window".into());
        }
        if let Some(reason) = super::input_route::pointer_reason(
            &op,
            before["input_framework"].as_str().unwrap_or("unknown"),
        ) {
            return BackgroundAttempt::Declined(reason.code().into());
        }
        if !super::cua_input::pointer_available() {
            return BackgroundAttempt::Declined(
                super::input_route::PhysicalReason::MissingAddressedApi
                    .code()
                    .into(),
            );
        }
        if !scene_worker {
            return BackgroundAttempt::Completed(rejected(
                json!({"error":"invalid_request","message":"Pointer input needs its scene owner"}),
                &op,
            ));
        }
        if let Err(error) = admit_scene(request, leased) {
            return BackgroundAttempt::Completed(rejected(error, &op));
        }
    }
    let mut input = json!({"operation":op,"pid":if keyboard {request["target"]["app"]["pid"].clone()} else {before["surface_owner"]["pid"].clone()},
        "window_id":request["target"]["native_id"],"deadline_unix_ms":request["deadline_unix_ms"],
        "input_framework":before["input_framework"].as_str().unwrap_or("unknown"),"native_modal":native_modal,
        "screen_x":x,"screen_y":y,"window_x":x-left,"window_y":y-top});
    for key in ["count", "button", "delta_x", "delta_y", "text", "keys"] {
        if let Some(value) = request.get(key) {
            input[key] = value.clone();
        }
    }
    if let Some(path) = request.get("path").and_then(Value::as_array) {
        input["path"] = Value::Array(path.iter().map(|p| json!([p["x"], p["y"]])).collect());
    }
    let mut target = BackgroundTarget {
        target: request["target"].clone(),
        surface_owner: before["surface_owner"].clone(),
        bounds: before["bounds"].clone(),
        keyboard,
        native_modal,
        foreground: request["_execution_foreground"] == true,
        deadline: request["deadline_unix_ms"].as_u64().unwrap_or(0),
        failures: RefCell::default(),
    };
    let _presentation = (!keyboard).then(|| {
        super::pointer_presentation::Scope::new(
            request,
            &before["bounds"],
            super::overlay::PointerRoute::Background,
        )
    });
    trace_route(request, "addressed", "pid_window_event");
    let mut result = super::cua_input::call(
        &input.to_string(),
        (&mut target as *mut BackgroundTarget).cast(),
        validate_background,
    );
    if result["transport"] == "ax_menu_equivalent" {
        trace_route(request, "native", "ax_menu_equivalent");
    }
    target.failures.borrow().apply(&mut result);
    if result["capability_evidence"] == "native_keyboard_semantics" {
        request["_keyboard_capability"] = result["capability_detail"].clone();
    }
    if let Some(error) = super::scroll_guard::failure() {
        if result["error"].is_object() {
            result["error"]["code"] = error["error"].clone();
            result["error"]["message"] = error["message"].clone();
            result["anchor_error"] = error;
        }
    }
    background_result(result)
}

fn dispatch(request: Value, leased: &mut bool, scene_worker: bool) -> Value {
    let _execution = super::execution_guard::Scope::new(&request, remaining(&request), || {
        TERMINATING.load(Ordering::Relaxed)
    });
    let proofs = super::associated_proof::Scope::new(scene_worker && *leased, remaining(&request));
    let mut result = dispatch_request(request, leased, scene_worker);
    proofs.finish(&mut result);
    if let Some(error) = super::execution_guard::failure() {
        result["error"] = error["error"].clone();
        result["message"] = error["message"].clone();
    }
    result
}

fn dispatch_request(mut request: Value, leased: &mut bool, scene_worker: bool) -> Value {
    let _anchor = super::scroll_guard::Scope::new(&request, remaining(&request), || {
        TERMINATING.load(Ordering::Relaxed)
    });
    let op = request["operation"].as_str().unwrap_or("").to_owned();
    let op = op.as_str();
    if op == "launch_app" {
        let Some(key) = request["application_key"]
            .as_str()
            .filter(|key| key.starts_with('/'))
        else {
            return rejected(
                json!({"error":"invalid_request","message":"Missing canonical launch target"}),
                op,
            );
        };
        let _coordination = match scene_lease::lock_coordination(
            scene_lease::InputCoordination::Foreground,
            remaining(&request),
        ) {
            Ok(lease) => lease,
            Err(message) => {
                return rejected(json!({"error":"native_input_busy","message":message}), op)
            }
        };
        let _launch = match scene_lease::lock_application(key, remaining(&request)) {
            Ok(lease) => lease,
            Err(message) => {
                return rejected(json!({"error":"native_input_busy","message":message}), op)
            }
        };
        if TERMINATING.load(Ordering::Relaxed) || remaining(&request).is_zero() {
            return rejected(
                json!({"error":"native_stopping","message":"Launch admission is closed"}),
                op,
            );
        }
        return native(&request, false);
    }
    if matches!(op, "keypress_app" | "focus_app") {
        let (Some(pid), Some(birth)) = (
            request["target"]["app"]["pid"]
                .as_u64()
                .and_then(|p| u32::try_from(p).ok()),
            request["target"]["app"]["generation"].as_u64(),
        ) else {
            return rejected(
                json!({"error":"invalid_request","message":"Missing application shortcut identity"}),
                op,
            );
        };
        let _coordination = match scene_lease::lock_coordination(
            scene_lease::InputCoordination::Foreground,
            remaining(&request),
        ) {
            Ok(v) => v,
            Err(message) => {
                return rejected(json!({"error":"native_input_busy","message":message}), op)
            }
        };
        let _instance = match scene_lease::lock_instance(pid, birth, remaining(&request)) {
            Ok(v) => v,
            Err(message) => {
                return rejected(json!({"error":"native_input_busy","message":message}), op)
            }
        };
        let _input = match scene_lease::lock_input(remaining(&request)) {
            Ok(v) => v,
            Err(message) => {
                return rejected(json!({"error":"native_input_busy","message":message}), op)
            }
        };
        if TERMINATING.load(Ordering::Relaxed) || remaining(&request).is_zero() {
            return rejected(
                json!({"error":"native_stopping","message":"Application shortcut admission closed"}),
                op,
            );
        }
        return native(&request, false);
    }
    if op == "prove_associated_surface" {
        if !scene_worker {
            return json!({"error":"invalid_request","message":"Association pixels require a short-lived Scene owner"});
        }
        if let Err(error) = admit_scene(&request, leased) {
            return error;
        }
        // This private read uses exact request identity and geometry directly;
        // it never inventories windows or recursively starts another helper.
        return native(&request, true);
    }
    if matches!(op, "capture" | "click" | "move" | "drag" | "scroll") && !scene_worker {
        return rejected(
            json!({"error":"invalid_request","message":"Scene operations require a short-lived scene owner"}),
            op,
        );
    }
    let focus_owner = if op == "release_focus" {
        let (mut pid, mut generation) = (0, 0);
        match unsafe {
            dtw_focus_owner(
                remaining(&request).as_millis().min(150) as u32,
                &mut pid,
                &mut generation,
            )
        } {
            0 => None,
            1 => Some((pid, generation)),
            _ => {
                return json!({"error":"focus_cleanup_unavailable","message":"Could not acquire original foreground ownership before cleanup"})
            }
        }
    } else {
        None
    };
    let instance = if input_operation(op) {
        let (Some(pid), Some(generation)) = (
            request["target"]["app"]["pid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok()),
            request["target"]["app"]["generation"].as_u64(),
        ) else {
            return rejected(
                json!({"error":"invalid_request","message":"Missing native app identity"}),
                op,
            );
        };
        Some((pid, generation))
    } else {
        focus_owner
    };
    // A directed route first shares coordination with independent background
    // mutations. Known foreground/return routes enter exclusively from the start.
    let known_foreground = matches!(op, "focus" | "begin_execution_focus") || focus_owner.is_some();
    let mut coordination = if instance.is_some() {
        let mode = if known_foreground {
            scene_lease::InputCoordination::Foreground
        } else {
            scene_lease::InputCoordination::Background
        };
        match scene_lease::lock_coordination(mode, remaining(&request)) {
            Ok(lease) => Some(lease),
            Err(message) => {
                return rejected(json!({"error":"native_input_busy","message":message}), op)
            }
        }
    } else {
        None
    };
    let mut instance_lease = if let Some((pid, generation)) = instance {
        let left = remaining(&request);
        if left.is_zero() || TERMINATING.load(Ordering::Relaxed) {
            return rejected(
                json!({"error":"deadline_exceeded","message":"Input deadline elapsed before instance admission"}),
                op,
            );
        }
        match scene_lease::lock_instance(pid, generation, left) {
            Ok(lease) => Some(lease),
            Err(message) => {
                return rejected(json!({"error":"native_input_busy","message":message}), op)
            }
        }
    } else {
        None
    };
    if TERMINATING.load(Ordering::Relaxed) {
        return rejected(
            json!({"error":"native_stopping","message":"Input admission is closed"}),
            op,
        );
    }
    if op == "begin_execution_focus" {
        let _input = match scene_lease::lock_input(remaining(&request)) {
            Ok(lease) => lease,
            Err(message) => {
                return rejected(json!({"error":"native_input_busy","message":message}), op)
            }
        };
        return native(&request, false);
    }
    let foreground_reason = match background(&mut request, leased, scene_worker) {
        BackgroundAttempt::Completed(result) => return result,
        BackgroundAttempt::Declined(reason) => Some(reason),
        BackgroundAttempt::NotApplicable if op == "focus" => Some("explicit_focus".to_owned()),
        BackgroundAttempt::NotApplicable => None,
    };
    if request["_defer_foreground"] == true && request["_execution_foreground"] != true {
        if let Some(reason) = &foreground_reason {
            return json!({"dispatch":"not_dispatched","evidence":"unverified",
                "foreground_plan":{"reason":reason,"visual":super::input_route::plan_visual(reason, op),
                    "capability_detail":request["_keyboard_capability"]}});
        }
    }
    let physical = matches!(
        op,
        "focus" | "type_text" | "keypress" | "scroll" | "click" | "move" | "drag"
    ) || (op == "release_focus" && focus_owner.is_some());
    if physical && !known_foreground {
        // A background preflight may have inspected an associated surface in
        // its proof child. Reap that Scene owner before waiting for exclusive
        // coordination: a peer can hold coordination while waiting for Scene.
        if let Err(error) = super::associated_proof::flush() {
            return rejected(error, op);
        }
        // background() declined before preparation/input. Never upgrade a shared
        // lease in place or wait for coordination while holding an instance lock:
        // a peer foreground transaction could be waiting for that same instance.
        drop(instance_lease.take());
        drop(coordination.take());
        coordination = match scene_lease::lock_coordination(
            scene_lease::InputCoordination::Foreground,
            remaining(&request),
        ) {
            Ok(lease) => Some(lease),
            Err(message) => {
                return rejected(json!({"error":"native_input_busy","message":message}), op)
            }
        };
        if let Some((pid, generation)) = instance {
            instance_lease = match scene_lease::lock_instance(pid, generation, remaining(&request))
            {
                Ok(lease) => Some(lease),
                Err(message) => {
                    return rejected(json!({"error":"native_input_busy","message":message}), op)
                }
            };
        }
    }
    if TERMINATING.load(Ordering::Relaxed) {
        return rejected(
            json!({"error":"native_stopping","message":"Input admission is closed"}),
            op,
        );
    }
    if physical && remaining(&request).is_zero() {
        return rejected(
            json!({"error":"deadline_exceeded","message":"Input deadline elapsed while waiting for native resources"}),
            op,
        );
    }
    // prepare_foreground / dispatch_owned revalidate the original native target
    // after the lock handover. Only an actual physical route takes the HID lease.
    let _input = if physical {
        match scene_lease::lock_input(remaining(&request)) {
            Ok(lease) => Some(lease),
            Err(message) => {
                return rejected(json!({"error":"native_input_busy","message":message}), op)
            }
        }
    } else {
        None
    };
    let pointer_borrow = if physical && matches!(op, "click" | "move" | "drag" | "scroll") {
        PointerBorrowTrace::begin(
            &request,
            foreground_reason.as_deref().unwrap_or("explicit_focus"),
        )
    } else {
        None
    };
    let mut borrowed = false;
    if physical && op != "release_focus" {
        trace_route(
            &request,
            if matches!(op, "click" | "move" | "drag" | "scroll") {
                "borrowed"
            } else {
                "activated"
            },
            foreground_reason.as_deref().unwrap_or("explicit_focus"),
        );
        // Cursor borrowing alone never grants application activation. Native
        // preparation and the eventual focus check both enforce this flag, so
        // losing foreground after preflight cannot turn hover into activation.
        request["_allow_activation"] = json!(
            request["_execution_foreground"] != true
                && super::input_route::allows_activation(
                    foreground_reason.as_deref().unwrap_or("explicit_focus")
                )
        );
        // Coordinate input need not depend on AX. Keyboard and explicit focus
        // retain exact-window and editing-destination validation on every down.
        request["visual"] = json!(super::input_route::visual_focus(op));
        let mut prepare = request.clone();
        prepare["operation"] = json!("prepare_foreground");
        // A hover is meaningful only while its borrowed physical pointer stays
        // on the target. Immediate restoration raced AppKit event processing
        // and dismissed hover UI before the ordinary read.
        if op == "move" {
            prepare.as_object_mut().unwrap().remove("screen_x");
            prepare.as_object_mut().unwrap().remove("screen_y");
        }
        let prepared = native(&prepare, false);
        if prepared.get("error").is_some() {
            let mut result = rejected(prepared, op);
            result["delivery"] = delivery(None, foreground_reason.as_deref(), false, None);
            return result;
        }
        borrowed = prepared["borrowed"].as_bool().unwrap_or(false);
        if matches!(op, "click" | "move" | "drag" | "scroll") {
            // Presentation uses the most recent native proof after focus/lease
            // handover. Input still performs its own target validation below.
            request["surface_owner"] = prepared["surface_owner"].clone();
            request["_pointer_bounds"] = prepared["bounds"].clone();
        }
    }
    let _presentation = matches!(op, "click" | "move" | "drag" | "scroll").then(|| {
        super::pointer_presentation::Scope::new(
            &request,
            &request["_pointer_bounds"],
            super::overlay::PointerRoute::Foreground,
        )
    });
    let call_deadline = request["deadline_unix_ms"].as_u64().unwrap_or(0);
    if matches!(op, "press" | "set_value") {
        trace_route(&request, "native", "ax_action");
    }
    let mut result = dispatch_owned(request, leased, scene_worker);
    if physical && op != "release_focus" {
        result["delivery"] =
            foreground_delivery(&result, op, foreground_reason.as_deref(), borrowed);
    } else if matches!(op, "press" | "set_value") {
        result["delivery"] = delivery(
            Some("semantic"),
            Some("native_semantic_action"),
            false,
            None,
        );
    }
    if physical && op != "focus" && op != "release_focus" {
        let cleanup = native(
            &json!({"operation":"release_input_focus",
                "deadline_unix_ms":super::focus_return_deadline_ms(now_ms(), Some(call_deadline))}),
            false,
        );
        record_foreground_cleanup(&mut result, cleanup);
    }
    // Explicit focus is an operation, not a script-wide lease: a later pixel
    // action runs in a sibling scene worker. Its final release_focus acquires
    // exclusive coordination anew. If another background operation outlives the
    // cleanup budget, cleanup stays unconfirmed rather than interrupting it.
    // Includes dispatch, paired input release, and foreground-return cleanup;
    // close before releasing the HID lease so a peer cannot enter this interval.
    drop(pointer_borrow);
    drop(_input);
    drop(instance_lease);
    drop(coordination);
    result
}
fn dispatch_owned(mut request: Value, leased: &mut bool, scene_worker: bool) -> Value {
    let op = request["operation"].as_str().unwrap_or("").to_owned();
    if !matches!(
        op.as_str(),
        "capture" | "click" | "move" | "drag" | "scroll"
    ) {
        return native(&request, false);
    }
    if !scene_worker {
        return rejected(
            json!({"error":"invalid_request",
            "message":"Scene operations require a short-lived scene owner"}),
            &op,
        );
    }
    let check = json!({"operation":"validate","target":request["target"],"deadline_unix_ms":request["deadline_unix_ms"]});
    let before = native(&check, false);
    if let Err(error) = validate_reply(&before, &request["target"]) {
        return rejected(error, &op);
    }
    if let Err(error) = admit_scene(&request, leased) {
        return rejected(error, &op);
    }
    request["pid"] = before["surface_owner"]["pid"].clone();
    request["surface_owner"] = before["surface_owner"].clone();
    request["window_id"] = request["target"]["native_id"].clone();
    let mut click_borrowed = None;
    if matches!(op.as_str(), "click" | "move" | "drag") {
        let mut focus = request.clone();
        focus["operation"] = json!("focus");
        let raised = native(&focus, false);
        click_borrowed = raised["borrowed"].as_bool();
        // This physical focus path proves process/CG ownership without needing
        // AX children. Failed or uncertain activation never becomes a click.
        if !click_can_follow_focus(&raised) {
            return raised;
        }
    }
    // Scroll establishes focus and posts its wheel event in the same input
    // transaction, but shares Scene.swift's SCK transparency proof. Keep SCK
    // in this short-lived owner and retain both scene/input leases until exit.
    let mut result = native(&request, op != "scroll");
    if let Some(borrowed) = click_borrowed {
        result["borrowed"] = json!(borrowed);
    }
    if matches!(op.as_str(), "click" | "move" | "drag" | "scroll") {
        return result;
    }
    if result.get("error").is_some() {
        return result;
    }
    let after = native(&check, false);
    if let Err(error) = validate_reply(&after, &request["target"]) {
        return error;
    }
    if let Some(error) = capture_snapshot_error(
        &before,
        &after,
        &result["bounds"],
        super::CaptureGeometryStage::Scene,
    ) {
        return error;
    }
    result["target"] = after["target"].clone();
    result
}

fn admit_scene(request: &Value, leased: &mut bool) -> Result<(), Value> {
    if !*leased {
        // Bootstrap/foreground preparation can prove an association before this
        // process has an SCK client. Its child must exit before taking this lock.
        super::associated_proof::flush()?;
        if stopping() {
            return Err(json!({"error":"native_stopping","message":"Scene admission is closed"}));
        }
        let left = remaining(request);
        if left.is_zero() {
            return Err(
                json!({"error":"deadline_exceeded","message":"Scene operation deadline expired before admission"}),
            );
        }
        let lease = scene_lease::lock_scene(left)
            .map_err(|message| json!({"error":"native_scene_busy","message":message}))?;
        // File ownership deliberately lasts until process exit/SCK teardown,
        // not merely until the asynchronous frame callback returns.
        std::mem::forget(lease);
        *leased = true;
    }
    if stopping() || remaining(request).is_zero() {
        return Err(
            json!({"error":if stopping() {"native_stopping"} else {"deadline_exceeded"},
            "message":"Scene admission ended before proof or input"}),
        );
    }
    super::associated_proof::use_local();
    Ok(())
}
fn rejected(mut result: Value, operation: &str) -> Value {
    if input_operation(operation) {
        result["dispatch"] = json!("not_dispatched");
        result["evidence"] = json!("unverified");
    }
    result
}
fn valid_bounds(bounds: &Value) -> bool {
    bounds.as_array().is_some_and(|a| {
        a.len() == 4
            && a.iter().all(|n| n.as_f64().is_some_and(|f| f.is_finite()))
            && a[2].as_f64().unwrap() > 0.0
            && a[3].as_f64().unwrap() > 0.0
    })
}

// Both snapshots have already passed validate_reply. Identity takes precedence
// over geometry: a replaced owner must never become a capture retry hint.
pub(super) fn capture_snapshot_error(
    before: &Value,
    after: &Value,
    frame_bounds: &Value,
    stage: super::CaptureGeometryStage,
) -> Option<Value> {
    if before["target"] != after["target"] || before["surface_owner"] != after["surface_owner"] {
        return Some(json!({"error":"target_changed",
            "message":"Window identity or surface owner changed during capture validation"}));
    }
    if !valid_bounds(frame_bounds) {
        return Some(json!({"error":"invalid_response","message":"Invalid capture frame bounds"}));
    }
    if before["bounds"] != after["bounds"] || frame_bounds != &after["bounds"] {
        return Some(stage.reply());
    }
    None
}

pub(super) fn validate_reply(reply: &Value, target: &Value) -> Result<(), Value> {
    if reply["error"].is_string() {
        return Err(reply.clone());
    }
    if !reply.is_object()
        || reply.get("error").is_some()
        || &reply["target"] != target
        || !target.is_object()
        || !valid_bounds(&reply["bounds"])
        || !reply["surface_owner"]["pid"]
            .as_u64()
            .is_some_and(|pid| pid > 0 && pid <= i32::MAX as u64)
        || !reply["surface_owner"]["generation"]
            .as_u64()
            .is_some_and(|birth| birth > 0)
    {
        return Err(
            json!({"error":"invalid_response","message":"Incomplete native target validation"}),
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_pixels_use_the_bound_native_action_bridge_once() {
        let request = json!({"operation":"click", "target":{"app":{"pid":7,"generation":8},"native_id":9},
            "deadline_unix_ms":123456,"screen_x":1,"screen_y":2});
        for reply in [
            json!({"native_menu":false}),
            json!({"native_menu":true,"dispatch":"dispatched","evidence":"unverified"}),
            json!({"native_menu":true,"dispatch":"unknown","evidence":"unverified","error":"ax_action_failed"}),
        ] {
            let calls = std::cell::Cell::new(0);
            let result = native_menu_click(&request, 42.5, -71.25, |sent, scene| {
                calls.set(calls.get() + 1);
                assert!(
                    !scene,
                    "AX menu actions belong to NativeStore, not the SCK extension"
                );
                assert_eq!(sent["operation"], "try_native_menu_click");
                assert_eq!(sent["target"], request["target"]);
                assert_eq!(sent["deadline_unix_ms"], request["deadline_unix_ms"]);
                assert_eq!(sent["screen_x"], 42.5);
                assert_eq!(sent["screen_y"], -71.25);
                reply.clone()
            });
            assert_eq!(
                result, reply,
                "No synthetic acknowledgement or replay after an AX result"
            );
            assert_eq!(calls.get(), 1);
        }
        assert_eq!(request["operation"], "click");
    }

    #[test]
    fn keyboard_trace_identifies_the_capability_without_text_or_key_payloads() {
        let request = json!({"operation":"keypress", "target":{"native_id":42},
            "_keyboard_capability":"appkit_multiline_text", "text":"private note", "keys":["META","W"]});
        let trace = route_trace(&request, "activated", "native_keyboard_semantics");
        assert_eq!(trace["capability_detail"], "appkit_multiline_text");
        assert_eq!(trace["operation"], "keypress");
        assert!(trace.get("text").is_none());
        assert!(trace.get("keys").is_none());
        assert!(route_trace(&request, "addressed", "pid_window_event")
            .get("capability_detail")
            .is_none());
    }

    #[test]
    fn pointer_borrow_trace_owns_bounded_metadata_and_unique_identity() {
        let request = json!({"operation":"move","target":{"native_id":42},
            "text":"private payload","path":[[1,2],[3,4]]});
        let first = PointerBorrowTrace::metadata(&request, "native_hover_tracking");
        let second = PointerBorrowTrace::metadata(&request, "native_hover_tracking");
        drop(request); // dispatch_owned can consume it before the end trace.
        assert_eq!(first["operation"], "move");
        assert_eq!(first["window"], 42);
        assert_eq!(first["pid"], std::process::id());
        assert_ne!(first["trace_id"], second["trace_id"]);
        assert_eq!(first.as_object().unwrap().len(), 6);
        assert!(first.get("text").is_none());
        assert!(first.get("path").is_none());
    }

    #[test]
    fn association_pixels_require_scene_role_and_live_admission() {
        let mut leased = false;
        let request =
            json!({"operation":"prove_associated_surface", "deadline_unix_ms":now_ms()+1000});
        let result = dispatch(request, &mut leased, false);
        assert_eq!(result["error"], "invalid_request");
        assert!(!leased);
        let result = dispatch(
            json!({"operation":"prove_associated_surface", "deadline_unix_ms":0}),
            &mut leased,
            true,
        );
        assert_eq!(result["error"], "deadline_exceeded");
        assert!(!leased);
    }

    #[test]
    fn click_never_bypasses_focus_change_or_unknown_effect() {
        assert!(click_can_follow_focus(&json!({"dispatch":"dispatched"})));
        for result in [
            json!({"dispatch":"not_dispatched","error":"semantic_window_unavailable"}),
            json!({"dispatch":"not_dispatched","error":"focus_changed"}),
            json!({"dispatch":"not_dispatched","error":"target_changed"}),
            json!({"dispatch":"unknown","error":"semantic_window_unavailable"}),
            json!({"dispatch":"dispatched","error":"focus_changed"}),
            json!({"dispatch":"unknown"}),
            json!({"dispatch":"not_dispatched"}),
            json!({}),
        ] {
            assert!(!click_can_follow_focus(&result));
        }
    }

    #[test]
    fn foreground_route_reports_actual_return_obligation() {
        for operation in ["focus", "click", "keypress", "scroll"] {
            for (prepared, actual) in [(false, false), (false, true), (true, true)] {
                let result = foreground_delivery(
                    &json!({"borrowed": actual}),
                    operation,
                    Some("native_preflight"),
                    prepared,
                );
                assert_eq!(result["route"], "foreground");
                assert_eq!(result["foreground_borrowed"], actual);
                assert_eq!(result["reason"], "native_preflight");
                if operation == "focus" {
                    assert_eq!(
                        result["focus_return"],
                        if actual { "pending" } else { "unchanged" }
                    );
                } else {
                    assert!(result.get("focus_return").is_none());
                }
            }
        }
        assert_eq!(
            foreground_delivery(&json!({}), "click", None, true)["foreground_borrowed"],
            true
        );
    }

    #[test]
    fn unchanged_cleanup_preserves_input_outcome_and_primary_errors() {
        let mut result = json!({"dispatch":"dispatched","evidence":"unverified",
            "delivery":foreground_delivery(&json!({"borrowed":false}), "click", None, false)});
        record_foreground_cleanup(&mut result, json!({"released":true,"focus":"unchanged"}));
        assert_eq!(result["dispatch"], "dispatched");
        assert_eq!(result["delivery"]["foreground_borrowed"], false);
        assert_eq!(result["delivery"]["focus_return"], "unchanged");
        assert!(result.get("error").is_none());
        assert!(result.get("cleanup_error").is_none());

        result["error"] = json!("input_failed");
        result["message"] = json!("original input error");
        result["dispatch"] = json!("unknown");
        record_foreground_cleanup(
            &mut result,
            json!({"error":"return_failed","message":"cleanup failed"}),
        );
        assert_eq!(result["error"], "input_failed");
        assert_eq!(result["message"], "original input error");
        assert_eq!(result["cleanup_error"]["error"], "return_failed");
        assert_eq!(result["delivery"]["focus_return"], "unconfirmed");
    }

    #[test]
    fn foreground_cleanup_failure_preserves_the_original_action_receipt() {
        let cleanup = json!({"error":"deadline_exceeded","message":"Return deadline elapsed"});
        for mut result in [
            json!({"dispatch":"dispatched","evidence":"unverified"}),
            json!({"dispatch":"unknown","evidence":"unverified",
                "error":"input_failed","message":"Original uncertain input"}),
            json!({"dispatch":"not_dispatched","evidence":"unverified",
                "error":"focus_changed","message":"Original admission rejection"}),
        ] {
            result["delivery"] = foreground_delivery(&result, "keypress", None, true);
            let original = result.clone();
            let original_outcome = super::super::action_reply(&original);
            record_foreground_cleanup(&mut result, cleanup.clone());

            assert_eq!(result["dispatch"], original["dispatch"]);
            assert_eq!(result.get("error"), original.get("error"));
            assert_eq!(result.get("message"), original.get("message"));
            assert_eq!(result["cleanup_error"], cleanup);
            assert_eq!(result["delivery"]["focus_return"], "unconfirmed");
            let outcome = super::super::action_reply(&result);
            assert_eq!(outcome.dispatch, original_outcome.dispatch);
            assert_eq!(outcome.error, original_outcome.error);
            assert_eq!(outcome.evidence, original_outcome.evidence);
            assert!(matches!(
                super::super::native_cleanup_error(&result),
                Some(crate::computer::Error::NativeFailure { code, message })
                    if code == "deadline_exceeded" && message == "Return deadline elapsed"
            ));
        }
    }

    #[test]
    fn modal_input_preflight_is_typed() {
        let modal = json!({"background_requires_foreground":true});
        assert_eq!(native_modal_input(&modal), Ok(true));
        assert_eq!(
            native_modal_input(&json!({"background_requires_foreground":false})),
            Ok(false)
        );
        for malformed in [
            json!({}),
            json!({"background_requires_foreground":1}),
            json!({"background_requires_foreground":null}),
            json!({"background_requires_foreground":"false"}),
        ] {
            assert_eq!(
                native_modal_input(&malformed).unwrap_err()["error"],
                "invalid_response"
            );
        }
    }

    #[test]
    fn only_undispatched_preflight_declines_allow_another_route() {
        let reason = "native_keyboard_semantics";
        let declined =
            background_result(json!({"dispatch":"not_dispatched","fallback_allowed":true,
            "capability_evidence":reason,"capability_detail":"chromium_supplementary_unicode",
            "error":{"code":"background_unavailable","message":reason}}));
        assert!(matches!(declined, BackgroundAttempt::Declined(value) if value == reason));
        for dispatch in ["dispatched", "unknown"] {
            let attempt = background_result(json!({"dispatch":dispatch,"fallback_allowed":true}));
            let BackgroundAttempt::Completed(result) = attempt else {
                panic!("uncertain input selected a second route")
            };
            assert_eq!(result["dispatch"], dispatch);
            assert_eq!(result["delivery"]["route"], "background");
            assert_eq!(result["delivery"]["foreground_borrowed"], false);
        }
    }

    #[test]
    fn capture_snapshot_classification_requires_stable_identity_and_valid_geometry() {
        let target = json!({"app":{"pid":7,"generation":3},"native_id":1,"generation":2});
        let before = json!({"target":target,"bounds":[-100,2,300,400],
            "surface_owner":{"pid":7,"generation":3}});
        for stage in [
            super::super::CaptureGeometryStage::Scene,
            super::super::CaptureGeometryStage::Resident,
        ] {
            assert!(capture_snapshot_error(&before, &before, &before["bounds"], stage).is_none());
            let moved_frame = json!([-99, 2, 300, 400]);
            let changed = capture_snapshot_error(&before, &before, &moved_frame, stage).unwrap();
            assert_eq!(
                super::super::CaptureGeometryStage::from_reply(&changed),
                Some(stage)
            );
            for bounds in [json!([]), json!([1, 2, 0, 4]), json!([1, 2, null, 4])] {
                let bad = capture_snapshot_error(&before, &before, &bounds, stage).unwrap();
                assert_eq!(bad["error"], "invalid_response");
                assert_eq!(super::super::CaptureGeometryStage::from_reply(&bad), None);
            }
            let mut replaced = before.clone();
            replaced["target"]["generation"] = json!(99);
            replaced["bounds"] = moved_frame.clone();
            let changed = capture_snapshot_error(&before, &replaced, &moved_frame, stage).unwrap();
            assert_eq!(changed["error"], "target_changed");
            assert_eq!(
                super::super::CaptureGeometryStage::from_reply(&changed),
                None
            );
        }
    }

    #[test]
    fn malformed_validation_never_enters_capture_or_click() {
        let target = json!({"app":{"pid":7},"native_id":1,"generation":2});
        for reply in [
            json!({}),
            json!({"target":target}),
            json!({"target":{},"bounds":[1,2,3,4]}),
            json!({"target":target,"bounds":[1,2,0,4]}),
            json!({"target":target,"bounds":[1,2,null,4]}),
        ] {
            assert!(validate_reply(&reply, &target).is_err());
        }
        assert!(
            validate_reply(&json!({"target":target,"bounds":[-100,2,300,400],"surface_owner":{"pid":7,"generation":1}}), &target).is_ok()
        );
    }

    #[test]
    fn keyboard_target_survives_geometry_changes_but_pointer_binding_does_not() {
        let identity = json!({"app":{"pid":7},"native_id":1,"generation":2});
        let mut target = BackgroundTarget {
            target: identity.clone(),
            surface_owner: json!({"pid":7,"generation":1}),
            bounds: json!([10, 20, 300, 400]),
            keyboard: true,
            native_modal: false,
            foreground: false,
            deadline: 0,
            failures: RefCell::default(),
        };
        let moved = json!({"target":identity,"bounds":[100,200,500,600],"surface_owner":{"pid":7,"generation":1},
            "background_requires_foreground":false});
        assert!(background_validation(&moved, &target, 0).is_ok());
        target.keyboard = false;
        assert!(background_validation(&moved, &target, 0).is_err());
        assert!(background_validation(&moved, &target, 1).is_ok());
        let replaced = json!({"target":{"app":{"pid":8},"native_id":1,"generation":2},"bounds":[10,20,300,400]});
        for keyboard in [false, true] {
            target.keyboard = keyboard;
            assert!(background_validation(&replaced, &target, 0).is_err());
        }
    }

    #[test]
    fn modal_transition_stops_background_input_without_selecting_a_second_route() {
        let identity = json!({"app":{"pid":7},"native_id":1,"generation":2});
        let mut target = BackgroundTarget {
            target: identity.clone(),
            surface_owner: json!({"pid":7,"generation":1}),
            bounds: json!([0, 0, 100, 100]),
            keyboard: true,
            native_modal: false,
            foreground: false,
            deadline: 0,
            failures: RefCell::default(),
        };
        let modal = json!({"target":identity,"bounds":target.bounds,
            "surface_owner":{"pid":7,"generation":1},"background_requires_foreground":true});
        for initially_modal in [false, true] {
            target.native_modal = initially_modal;
            let mut initial = modal.clone();
            initial["background_requires_foreground"] = json!(initially_modal);
            assert!(background_validation(&initial, &target, 0).is_ok());
            let mut changed = initial.clone();
            changed["background_requires_foreground"] = json!(!initially_modal);
            let error = background_validation(&changed, &target, 0).unwrap_err();
            assert_eq!(error["error"], "background_route_changed");
            assert!(background_validation(&changed, &target, 1).is_ok());
            let mut clear = initial.clone();
            assert!(background_validation(&clear, &target, 0).is_ok());
            clear
                .as_object_mut()
                .unwrap()
                .remove("background_requires_foreground");
            assert_eq!(
                background_validation(&clear, &target, 0).unwrap_err()["error"],
                "invalid_response"
            );
            assert!(background_validation(&clear, &target, 1).is_ok());
            target.keyboard = false;
            assert!(background_validation(&clear, &target, 0).is_ok());
            target.keyboard = true;
            for dispatch in ["not_dispatched", "unknown"] {
                let attempt = background_result(json!({"error":error, "dispatch":dispatch,
                    "fallback_allowed":false}));
                let BackgroundAttempt::Completed(result) = attempt else {
                    panic!(
                        "a modal transition after route selection must not select a second route"
                    );
                };
                assert_eq!(result["dispatch"], dispatch);
            }
        }
    }

    #[test]
    fn native_validation_cause_survives_callback_and_cleanup() {
        let first = json!({"error":"ax_unavailable","message":"AX attribute AXModal failed: -25204",
            "details":{"attribute":"AXModal","ax_error":-25204}});
        let later = json!({"error":"target_changed","message":"Window disappeared"});
        let mut failures = BackgroundValidationFailures::default();
        failures.record(0, first.clone());
        failures.record(0, later.clone());
        failures.record(2, later.clone());
        let mut reply = json!({"ok":false,"dispatch":"unknown","fallback_allowed":false,
            "error":{"code":"background_input_failed","message":"input target changed"},
            "cleanup_error":{"code":"cleanup_failed","message":"release failed"}});
        failures.apply(&mut reply);
        assert_eq!(reply["dispatch"], "unknown");
        assert_eq!(reply["fallback_allowed"], false);
        assert_eq!(reply["error"]["code"], "background_input_failed");
        assert_eq!(
            reply["error"]["message"],
            "ax_unavailable: AX attribute AXModal failed: -25204"
        );
        assert_eq!(reply["validation_error"], first);
        assert_eq!(reply["cleanup_validation_error"], later);
        assert_eq!(
            reply["cleanup_error"]["message"],
            "target_changed: Window disappeared"
        );
    }

    #[test]
    fn focus_loss_keeps_its_code_without_changing_partial_dispatch() {
        let mut failures = BackgroundValidationFailures::default();
        failures.record(
            0,
            json!({"error":"focus_changed","message":"Execution scope invalidated"}),
        );
        let mut reply = json!({"dispatch":"unknown","fallback_allowed":false,
            "error":{"code":"background_input_failed","message":"callback denied"}});
        failures.apply(&mut reply);
        assert_eq!(reply["error"]["code"], "focus_changed");
        assert_eq!(reply["dispatch"], "unknown");
        assert_eq!(reply["fallback_allowed"], false);
    }

    #[test]
    fn resident_ax_worker_never_acquires_a_scene_or_dispatches_pixel_input() {
        for operation in ["capture", "click", "scroll"] {
            let mut leased = false;
            let reply = dispatch(json!({"operation":operation}), &mut leased, false);
            assert_eq!(reply["error"], "invalid_request");
            assert!(!leased);
            if input_operation(operation) {
                assert_eq!(reply["dispatch"], "not_dispatched");
            }
        }
    }
}
// Keep blocking pipe reads off the Cocoa main thread. Otherwise an activation
// notification can sit behind stdin and invalidate a later execution epoch.
fn read_requests(input: &mut impl BufRead, mut publish: impl FnMut(Vec<u8>) -> bool) {
    loop {
        let mut bytes = Vec::new();
        let size = (&mut *input)
            .take(64 * 1024 + 2)
            .read_until(b'\n', &mut bytes)
            .unwrap_or(0);
        if size == 0 || size > 64 * 1024 + 1 || bytes.last() != Some(&b'\n') || !publish(bytes) {
            break;
        }
    }
}
fn next_request(
    requests: &std::sync::mpsc::Receiver<Vec<u8>>,
    mut pump: impl FnMut(),
    stopped: impl Fn() -> bool,
) -> Option<Vec<u8>> {
    loop {
        if stopped() {
            return None;
        }
        pump();
        match requests.recv_timeout(Duration::from_millis(20)) {
            Ok(bytes) => {
                // Drain already queued workspace history before arming or
                // validating an execution, even when another frame is ready.
                pump();
                return Some(bytes);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}
fn pump_workspace_notifications() {
    core_foundation::runloop::CFRunLoop::run_in_mode(
        unsafe { core_foundation::runloop::kCFRunLoopDefaultMode },
        Duration::from_millis(1),
        false,
    );
}

/// Called at executable startup on its main thread for private worker processes.
pub fn worker_entry() -> bool {
    let scene_worker = match std::env::args().nth(1).as_deref() {
        Some("--dtw-native-worker") => false,
        Some("--dtw-scene-worker") => true,
        _ => return false,
    };
    unsafe {
        dtw_set_pointer_presenter(Some(super::pointer_presentation::native_posted));
        dtw_set_associated_surface_prover(
            Some(super::associated_proof::callback),
            Some(super::associated_proof::free),
        );
    }
    if install_termination_handler().is_err() {
        std::process::exit(1);
    }
    let parent = unsafe { libc::getppid() };
    if parent <= 1 {
        std::process::exit(1);
    }
    std::thread::spawn(move || loop {
        if TERMINATING.load(Ordering::Relaxed) || unsafe { libc::getppid() } != parent {
            shutdown_worker();
        }
        std::thread::sleep(Duration::from_millis(10));
    });
    let (publish, requests) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        read_requests(&mut stdin.lock(), |bytes| publish.send(bytes).is_ok());
    });
    let mut output = std::io::stdout().lock();
    let mut leased = false;
    while let Some(bytes) = next_request(&requests, pump_workspace_notifications, stopping) {
        let request: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => break,
        };
        let request_id = request["request_id"].clone();
        if TERMINATING.load(Ordering::Relaxed) {
            break;
        }
        let result =
            json!({"request_id":request_id,"result":dispatch(request,&mut leased,scene_worker)});
        if writeln!(output, "{result}")
            .and_then(|_| output.flush())
            .is_err()
        {
            break;
        }
    }
    // EOF, malformed input and broken output use the same closed admission and
    // input drain as SIGTERM and parent death.
    shutdown_worker()
}

#[cfg(test)]
mod workspace_dispatch_tests {
    use super::*;
    #[test]
    fn idle_notifications_are_serviced_before_the_next_request() {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let mut pumps = 0;
        let until = Instant::now() + Duration::from_secs(1);
        let frame = next_request(
            &receive,
            || {
                pumps += 1;
                if pumps == 3 {
                    send.send(b"ready\n".to_vec()).unwrap();
                }
            },
            || Instant::now() > until,
        );
        assert_eq!(frame, Some(b"ready\n".to_vec()));
        assert!(
            pumps >= 3,
            "No new request may be needed to deliver pending history"
        );
    }
    #[test]
    fn request_reader_preserves_frame_bounds_eof_and_backpressure() {
        for (input, expected) in [
            (b"one\ntwo\n".to_vec(), 2),
            (b"incomplete".to_vec(), 0),
            ([vec![b'x'; 64 * 1024 + 1], vec![b'\n']].concat(), 0),
        ] {
            let mut frames = Vec::new();
            read_requests(&mut std::io::Cursor::new(input), |frame| {
                frames.push(frame);
                true
            });
            assert_eq!(frames.len(), expected);
        }
        let mut count = 0;
        read_requests(&mut std::io::Cursor::new(b"one\ntwo\n"), |_| {
            count += 1;
            false
        });
        assert_eq!(count, 1);
    }
    #[test]
    fn stopped_request_wait_never_admits_a_ready_frame() {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        send.send(vec![1]).unwrap();
        assert!(next_request(
            &receive,
            || panic!("stopping does not dispatch callbacks"),
            || true
        )
        .is_none());
    }
}

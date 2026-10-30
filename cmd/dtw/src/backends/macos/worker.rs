//! Private worker entry; sequential native ownership and a parent-death watchdog.
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    ffi::{c_void, CStr, CString},
    io::{BufRead, Read, Write},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[path = "../../scene_lease.rs"]
mod scene_lease;
unsafe extern "C" {
    fn dtw_native_call(request: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    fn dtw_native_free(reply: *mut std::ffi::c_char);
    fn dtw_set_pointer_presenter(callback: Option<extern "C" fn(f64, f64, i32)>);
    fn dtw_scene_call(request: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    fn dtw_scene_free(reply: *mut std::ffi::c_char);
    fn dtw_input_quiesce(timeout_milliseconds: u32) -> bool;
    fn dtw_focus_quiesce(timeout_milliseconds: u32) -> bool;
    fn dtw_focus_owner(timeout_milliseconds: u32, pid: *mut u32, generation: *mut u64) -> i32;
}

static TERMINATING: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_STARTED: AtomicBool = AtomicBool::new(false);

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

fn native(request: &Value, scene: bool) -> Value {
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
        "press" | "set_value" | "focus" | "type_text" | "keypress" | "scroll" | "click"
    )
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn remaining(request: &Value) -> Duration {
    Duration::from_millis(
        request["deadline_unix_ms"]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(now_ms()),
    )
}

struct BackgroundTarget {
    target: Value,
    bounds: Value,
    keyboard: bool,
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
        // Only owned key-up cleanup uses this phase. A closing window must not
        // strand keys in its still-running app; a reused PID is never accepted.
        let reply = native(
            &json!({"operation":"validate_input_owner", "app":target.target["app"],
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
    // Mouse releases and focus restoration retain the original window proof.
    // Cleanup ignores normal cancellation, focus and geometry changes.
    let check = json!({"operation":if phase == 0 {"validate_background"} else {"validate"},
        "target":target.target,"keyboard":target.keyboard,"step":phase == 0,
        "deadline_unix_ms":if phase == 0 {target.deadline} else {now_ms()+150}});
    let reply = native(&check, false);
    match background_validation(&reply, target, phase) {
        Ok(()) => 1,
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

fn foreground_required(reason: &str) -> Value {
    let mut result = rejected(
        json!({"error":"foreground_required",
        "message":"This action requires foreground input; the service is configured for background_only"}),
        "focus",
    );
    result["delivery"] = delivery(None, Some(reason), false, None);
    result
}

/// A decline is an explicit, side-effect-free preflight result. An unknown or
/// dispatched result never selects another route, even with a malformed flag.
fn background_result(mut result: Value) -> BackgroundAttempt {
    if result["fallback_allowed"] == true && result["dispatch"] == "not_dispatched" {
        return BackgroundAttempt::Declined(
            result["error"]["message"]
                .as_str()
                .unwrap_or("background_route_unavailable")
                .to_owned(),
        );
    }
    if result["error"].is_object() {
        result["message"] = result["error"]["message"].clone();
        result["error"] = result["error"]["code"].clone();
    }
    result["evidence"] = json!("unverified");
    result["route"] = json!("cua_pid_window");
    result["delivery"] = delivery(
        Some("background"),
        Some("background_supported"),
        false,
        None,
    );
    BackgroundAttempt::Completed(result)
}

fn background(request: &mut Value) -> BackgroundAttempt {
    let op = request["operation"].as_str().unwrap_or("").to_owned();
    if !matches!(op.as_str(), "click" | "scroll" | "type_text" | "keypress") {
        return BackgroundAttempt::NotApplicable;
    }
    let keyboard = matches!(op.as_str(), "type_text" | "keypress");
    let check = json!({"operation":"validate_background","target":request["target"],
        "keyboard":keyboard,"consume":true,"deadline_unix_ms":request["deadline_unix_ms"]});
    let before = native(&check, false);
    if let Err(error) = validate_reply(&before, &request["target"]) {
        return BackgroundAttempt::Completed(rejected(error, &op));
    }
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
        request["screen_x"] = json!(x);
        request["screen_y"] = json!(y);
        request["_pointer_bounds"] = before["bounds"].clone();
    }
    let mut input = json!({"operation":op,"pid":request["target"]["app"]["pid"],
        "window_id":request["target"]["native_id"],"deadline_unix_ms":request["deadline_unix_ms"],
        "chromium":before["chromium"].as_bool().unwrap_or(false),
        "screen_x":x,"screen_y":y,"window_x":x-left,"window_y":y-top});
    for key in ["count", "button", "delta_x", "delta_y", "text", "keys"] {
        if let Some(value) = request.get(key) {
            input[key] = value.clone();
        }
    }
    let mut target = BackgroundTarget {
        target: request["target"].clone(),
        bounds: before["bounds"].clone(),
        keyboard,
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
    let mut result = super::cua_input::call(
        &input.to_string(),
        (&mut target as *mut BackgroundTarget).cast(),
        validate_background,
    );
    target.failures.borrow().apply(&mut result);
    if let Some(error) = super::scroll_guard::failure() {
        if result["error"].is_object() {
            result["error"]["code"] = error["error"].clone();
            result["error"]["message"] = error["message"].clone();
            result["anchor_error"] = error;
        }
    }
    background_result(result)
}

fn dispatch(mut request: Value, leased: &mut bool, scene_worker: bool) -> Value {
    let _anchor = super::scroll_guard::Scope::new(&request, remaining(&request), || {
        TERMINATING.load(Ordering::Relaxed)
    });
    let op = request["operation"].as_str().unwrap_or("").to_owned();
    let op = op.as_str();
    let background_only = match request["input_policy"].as_str() {
        Some("background_only") => true,
        None | Some("auto") => false,
        _ => {
            return rejected(
                json!({"error":"invalid_request","message":"Unknown service input policy"}),
                op,
            )
        }
    };
    if background_only && op == "focus" {
        return foreground_required("explicit_focus");
    }
    if matches!(op, "capture" | "click" | "scroll") && !scene_worker {
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
    let known_foreground = op == "focus" || focus_owner.is_some();
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
    let foreground_reason = match background(&mut request) {
        BackgroundAttempt::Completed(result) => return result,
        BackgroundAttempt::Declined(reason) => Some(reason),
        BackgroundAttempt::NotApplicable if op == "focus" => Some("explicit_focus".to_owned()),
        BackgroundAttempt::NotApplicable => None,
    };
    if background_only {
        if let Some(reason) = &foreground_reason {
            return foreground_required(reason);
        }
    }
    let physical = matches!(op, "focus" | "type_text" | "keypress" | "scroll" | "click")
        || (op == "release_focus" && focus_owner.is_some());
    if physical && !known_foreground {
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
    let mut borrowed = false;
    if physical && op != "release_focus" {
        let mut prepare = request.clone();
        prepare["operation"] = json!("prepare_foreground");
        let prepared = native(&prepare, false);
        if prepared.get("error").is_some() {
            let mut result = rejected(prepared, op);
            result["delivery"] = delivery(None, foreground_reason.as_deref(), false, None);
            return result;
        }
        borrowed = prepared["borrowed"].as_bool().unwrap_or(false);
    }
    let _presentation = matches!(op, "click" | "scroll").then(|| {
        super::pointer_presentation::Scope::new(
            &request,
            &request["_pointer_bounds"],
            super::overlay::PointerRoute::Foreground,
        )
    });
    let mut result = dispatch_owned(request, leased, scene_worker);
    if physical && op != "release_focus" {
        result["delivery"] = delivery(
            Some("foreground"),
            foreground_reason.as_deref(),
            borrowed,
            if op == "focus" { Some("pending") } else { None },
        );
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
            &json!({"operation":"release_focus","deadline_unix_ms":now_ms()+300}),
            false,
        );
        result["delivery"]["focus_return"] = if cleanup.get("error").is_some() {
            json!("unconfirmed")
        } else {
            cleanup["focus"].clone()
        };
        if cleanup.get("error").is_some() {
            result["cleanup_error"] = cleanup.clone();
            if result.get("error").is_none() {
                result["error"] = cleanup["error"].clone();
                result["message"] = cleanup["message"].clone();
                result["dispatch"] = json!("unknown");
            }
        }
    }
    // Explicit focus is an operation, not a script-wide lease: a later pixel
    // action runs in a sibling scene worker. Its final release_focus acquires
    // exclusive coordination anew. If another background operation outlives the
    // cleanup budget, cleanup stays unconfirmed rather than interrupting it.
    drop(_input);
    drop(instance_lease);
    drop(coordination);
    result
}
fn dispatch_owned(mut request: Value, leased: &mut bool, scene_worker: bool) -> Value {
    let op = request["operation"].as_str().unwrap_or("").to_owned();
    if !matches!(op.as_str(), "capture" | "click" | "scroll") {
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
    if !*leased {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let left = request["deadline_unix_ms"]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(now as u64);
        if left == 0 {
            return rejected(
                json!({"error":"deadline_exceeded","message":"Scene operation deadline expired before admission"}),
                &op,
            );
        }
        match scene_lease::lock_scene(Duration::from_millis(left)) {
            Ok(lease) => {
                std::mem::forget(lease);
                *leased = true;
            }
            Err(message) => {
                return rejected(json!({"error":"native_scene_busy","message":message}), &op)
            }
        }
    }
    request["pid"] = request["target"]["app"]["pid"].clone();
    request["window_id"] = request["target"]["native_id"].clone();
    if op == "click" {
        let mut focus = request.clone();
        focus["operation"] = json!("focus");
        let raised = native(&focus, false);
        // Preserve the plain visible-point path for surfaces without AX. A
        // possibly dispatched focus attempt is never followed by another route.
        if raised.get("error").is_some() && raised["dispatch"] != "not_dispatched" {
            return raised;
        }
    }
    // Scroll establishes focus and posts its wheel event in the same input
    // transaction, but shares Scene.swift's SCK transparency proof. Keep SCK
    // in this short-lived owner and retain both scene/input leases until exit.
    let mut result = native(&request, op != "scroll");
    if op == "click" || op == "scroll" {
        return result;
    }
    if result.get("error").is_some() {
        return result;
    }
    let after = native(&check, false);
    if let Err(error) = validate_reply(&after, &request["target"]) {
        return error;
    }
    if before["bounds"] != after["bounds"] || result["bounds"] != after["bounds"] {
        return json!({"error":"stale_observation","message":"Window moved during capture"});
    }
    result["target"] = after["target"].clone();
    result
}
fn rejected(mut result: Value, operation: &str) -> Value {
    if input_operation(operation) {
        result["dispatch"] = json!("not_dispatched");
        result["evidence"] = json!("unverified");
    }
    result
}
pub(super) fn validate_reply(reply: &Value, target: &Value) -> Result<(), Value> {
    if reply["error"].is_string() {
        return Err(reply.clone());
    }
    let bounds_valid = reply["bounds"].as_array().is_some_and(|a| {
        a.len() == 4
            && a.iter().all(|n| n.as_f64().is_some_and(|f| f.is_finite()))
            && a[2].as_f64().unwrap() > 0.0
            && a[3].as_f64().unwrap() > 0.0
    });
    if !reply.is_object()
        || reply.get("error").is_some()
        || &reply["target"] != target
        || !target.is_object()
        || !bounds_valid
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
    fn strict_focus_rejects_before_any_native_target_read_or_preparation() {
        // Deliberately no native target: this must not call AX, take a lease,
        // prepare foreground, or dispatch an event even without GUI permissions.
        let mut leased = false;
        let reply = dispatch(
            json!({"operation":"focus","input_policy":"background_only"}),
            &mut leased,
            false,
        );
        assert_eq!(reply["dispatch"], "not_dispatched");
        assert_eq!(reply["error"], "foreground_required");
        assert_eq!(reply["delivery"]["foreground_borrowed"], false);
        assert!(reply["delivery"]["route"].is_null());
        assert!(!leased);
    }

    #[test]
    fn only_undispatched_preflight_declines_allow_another_route() {
        let reason = "Unicode payload requires foreground input";
        let declined =
            background_result(json!({"dispatch":"not_dispatched","fallback_allowed":true,
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
            validate_reply(&json!({"target":target,"bounds":[-100,2,300,400]}), &target).is_ok()
        );
    }

    #[test]
    fn keyboard_target_survives_geometry_changes_but_pointer_binding_does_not() {
        let identity = json!({"app":{"pid":7},"native_id":1,"generation":2});
        let mut target = BackgroundTarget {
            target: identity.clone(),
            bounds: json!([10, 20, 300, 400]),
            keyboard: true,
            deadline: 0,
            failures: RefCell::default(),
        };
        let moved = json!({"target":identity,"bounds":[100,200,500,600]});
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
/// Called at executable startup on its main thread for private worker processes.
pub fn worker_entry() -> bool {
    let scene_worker = match std::env::args().nth(1).as_deref() {
        Some("--dtw-native-worker") => false,
        Some("--dtw-scene-worker") => true,
        _ => return false,
    };
    unsafe {
        dtw_set_pointer_presenter(Some(super::pointer_presentation::native_posted));
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
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut output = std::io::stdout().lock();
    let mut leased = false;
    loop {
        // Cancellation can arrive during dispatch. Reach the shutdown run loop
        // before blocking on another request, even while the parent retains stdin.
        if TERMINATING.load(Ordering::Relaxed) {
            break;
        }
        let mut bytes = Vec::new();
        let size = (&mut input)
            .take(64 * 1024 + 2)
            .read_until(b'\n', &mut bytes)
            .unwrap_or(0);
        if size == 0 {
            break;
        }
        if size > 64 * 1024 + 1 || bytes.last() != Some(&b'\n') {
            break;
        }
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

//! Private worker entry; sequential native ownership and a parent-death watchdog.
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    io::{BufRead, Read, Write},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[path = "../../scene_lease.rs"]
mod scene_lease;
unsafe extern "C" {
    fn dtw_native_call(request: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    fn dtw_native_free(reply: *mut std::ffi::c_char);
    fn dtw_scene_call(request: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    fn dtw_scene_free(reply: *mut std::ffi::c_char);
    fn dtw_input_quiesce(timeout_milliseconds: u32) -> bool;
}

static TERMINATING: AtomicBool = AtomicBool::new(false);

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

fn shutdown_worker() -> ! {
    TERMINATING.store(true, Ordering::Relaxed);
    // Also bounded when the parent has died and can no longer enforce its
    // 500 ms grace. Reads never enter the input gate and cannot delay this drain.
    let clean = unsafe { dtw_input_quiesce(400) };
    if !clean {
        eprintln!("dtw native input cleanup did not complete before shutdown");
    }
    std::process::exit(if clean {
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
fn dispatch(mut request: Value, leased: &mut bool, scene_worker: bool) -> Value {
    let op = request["operation"].as_str().unwrap_or("").to_owned();
    if op != "capture" && op != "click" {
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
    let mut result = native(&request, true);
    if op == "click" {
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
    if operation == "click" {
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
    fn resident_ax_worker_never_acquires_a_scene_or_dispatches_pixel_input() {
        for operation in ["capture", "click"] {
            let mut leased = false;
            let reply = dispatch(json!({"operation":operation}), &mut leased, false);
            assert_eq!(reply["error"], "invalid_request");
            assert!(!leased);
            if operation == "click" {
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

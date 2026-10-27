//! Private worker entry; sequential native ownership and a parent-death watchdog.
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    io::{BufRead, Read, Write},
    time::Duration,
};
#[path = "../../scene_lease.rs"]
mod scene_lease;
unsafe extern "C" {
    fn dtw_native_call(request: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    fn dtw_native_free(reply: *mut std::ffi::c_char);
    fn dtw_scene_call(request: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    fn dtw_scene_free(reply: *mut std::ffi::c_char);
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
fn dispatch(mut request: Value, leased: &mut bool) -> Value {
    let op = request["operation"].as_str().unwrap_or("").to_owned();
    if op != "capture" && op != "click" {
        return native(&request, false);
    }
    let check = json!({"operation":"validate","target":request["target"],"deadline_unix_ms":request["deadline_unix_ms"]});
    let before = native(&check, false);
    if let Err(error) = validate_reply(&before, &request["target"]) {
        return rejected(error, &op);
    }
    if !*leased {
        match scene_lease::lock_scene() {
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
fn validate_reply(reply: &Value, target: &Value) -> Result<(), Value> {
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
}
/// Called at executable startup on its main thread for private worker processes.
pub fn worker_entry() -> bool {
    if std::env::args().nth(1).as_deref() != Some("--dtw-native-worker") {
        return false;
    }
    let parent = unsafe { libc::getppid() };
    if parent <= 1 {
        std::process::exit(1);
    }
    std::thread::spawn(move || loop {
        if unsafe { libc::getppid() } != parent {
            std::process::exit(1);
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
        let result = json!({"request_id":request_id,"result":dispatch(request,&mut leased)});
        if writeln!(output, "{result}")
            .and_then(|_| output.flush())
            .is_err()
        {
            break;
        }
    }
    true
}

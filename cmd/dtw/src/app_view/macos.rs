//! Public macOS APIs in the independently maintained DTW bridge.
use serde_json::Value;
use std::ffi::{CStr, CString};
unsafe extern "C" {
    fn dtw_scene_call(request: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    fn dtw_scene_free(reply: *mut std::ffi::c_char);
}
pub(super) fn call(request: Value) -> Result<Value, String> {
    let request = CString::new(request.to_string()).map_err(|e| e.to_string())?;
    unsafe {
        let reply = dtw_scene_call(request.as_ptr());
        if reply.is_null() {
            return Err("macOS extension returned no response".into());
        }
        let result =
            serde_json::from_slice(CStr::from_ptr(reply).to_bytes()).map_err(|e| e.to_string());
        dtw_scene_free(reply);
        result
    }
}

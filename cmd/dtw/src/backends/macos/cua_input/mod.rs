//! Minimal CUA native primitives, with DTW-owned identity and execution lifetime.
mod ax;
mod control;
mod keyboard;
mod mouse;
mod skylight;
pub use control::Validate;
use serde::Deserialize;
use serde_json::Value;
use std::ffi::{c_char, c_void, CStr, CString};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default)]
    chromium: bool,
    operation: String,
    pid: i32,
    window_id: u32,
    deadline_unix_ms: u64,
    #[serde(default)]
    screen_x: f64,
    #[serde(default)]
    screen_y: f64,
    #[serde(default)]
    window_x: f64,
    #[serde(default)]
    window_y: f64,
    #[serde(default = "one")]
    count: u32,
    #[serde(default = "left")]
    button: String,
    #[serde(default)]
    delta_x: i32,
    #[serde(default)]
    delta_y: i32,
    #[serde(default)]
    text: String,
    #[serde(default)]
    keys: Vec<String>,
}
fn one() -> u32 {
    1
}
fn left() -> String {
    "left".into()
}
impl Request {
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.pid > 0 && self.window_id != 0, "invalid input target");
        anyhow::ensure!(
            [self.screen_x, self.screen_y, self.window_x, self.window_y]
                .iter()
                .all(|v| v.is_finite()),
            "non-finite input point"
        );
        match self.operation.as_str() {
            "click" => {
                anyhow::ensure!(
                    (1..=2).contains(&self.count)
                        && matches!(self.button.as_str(), "left" | "right"),
                    "unsupported click count or button"
                );
            }
            "scroll" => {
                anyhow::ensure!(
                    self.delta_x.unsigned_abs() <= 100 && self.delta_y.unsigned_abs() <= 100,
                    "scroll step limit exceeded"
                );
            }
            "type_text" => {}
            "keypress" => {
                let (key, _) = self.key_chord()?;
                keyboard::key_name_to_code(key)?;
            }
            _ => anyhow::bail!("unsupported background input operation"),
        }
        Ok(())
    }
    fn key_chord(&self) -> anyhow::Result<(&str, Vec<&str>)> {
        anyhow::ensure!(self.keys.len() <= 5, "key chord exceeds five keys");
        let (key, modifiers) = self
            .keys
            .split_last()
            .ok_or_else(|| anyhow::anyhow!("empty key chord"))?;
        let mut seen = std::collections::HashSet::new();
        for modifier in modifiers {
            anyhow::ensure!(
                keyboard::is_modifier(modifier),
                "unsupported modifier {modifier}"
            );
            anyhow::ensure!(
                seen.insert(keyboard::key_name_to_code(modifier)?),
                "duplicate chord modifier"
            );
        }
        if keyboard::is_modifier(key) {
            anyhow::ensure!(
                seen.insert(keyboard::key_name_to_code(key)?),
                "duplicate chord modifier"
            );
        }
        Ok((key, modifiers.iter().map(String::as_str).collect()))
    }
    fn keyboard(&self) -> bool {
        matches!(self.operation.as_str(), "type_text" | "keypress")
    }
    fn foreground_requirement(&self) -> Option<&'static str> {
        // The donor only encodes a single right click.
        if self.operation == "click" && self.button == "right" && self.count == 2 {
            return Some("right double click requires foreground delivery");
        }
        // Background TextEdit receives ordinary text but can ignore Return
        // inside a multiline request. Select the whole control-bearing request
        // before posting any prefix; never repair it with a replay.
        if self.operation == "type_text" && self.text.contains(['\n', '\r', '\t']) {
            return Some("text control characters require foreground delivery");
        }
        // Chromium can receive a keydown without multi-unit text insertion.
        // Decide for the whole request before priming: a delivered prefix must
        // never be replayed. Standard menu equivalents need AppKit dispatch in
        // other apps too (TextEdit Cmd+W posted successfully but did not close).
        if self.chromium
            && self.operation == "type_text"
            && self.text.chars().any(|ch| ch.len_utf16() > 1)
        {
            return Some("Chromium supplementary Unicode text requires foreground delivery");
        }
        if self.operation == "keypress" {
            let (key, modifiers) = self.key_chord().ok()?;
            if matches!(keyboard::key_name_to_code(key).ok(), Some(36 | 48 | 76)) {
                return Some("Return and Tab require foreground delivery");
            }
            let mut chars = key.trim().chars();
            if let (Some(ch), None) = (chars.next(), chars.next()) {
                if !ch.is_whitespace()
                    && !ch.is_control()
                    && modifiers
                        .iter()
                        .any(|modifier| keyboard::key_name_to_code(modifier).ok() == Some(55))
                {
                    return Some("AppKit menu key equivalents require foreground delivery");
                }
            }
        }
        None
    }
}
pub fn call(json: &str, context: *mut c_void, validate: Validate) -> Value {
    let request: Request = match serde_json::from_str(json) {
        Ok(request) => request,
        Err(error) => return control::rejected("invalid_arguments", &error.to_string(), false),
    };
    if let Err(error) = request.validate() {
        return control::rejected("invalid_arguments", &error.to_string(), false);
    }
    // Missing background semantics are known before any native preparation or
    // input. The worker can then select its single foreground/HID attempt.
    if let Some(reason) = request.foreground_requirement() {
        return control::rejected("background_unavailable", reason, true);
    }
    let _pool = AutoreleasePool::new();
    let session = match control::Session::start(
        request.pid,
        request.deadline_unix_ms,
        context,
        validate,
        !request.chromium,
    ) {
        Ok(session) => session,
        Err(error) => return error,
    };
    if let Err(error) = control::check() {
        return session.finish(Err(error), false);
    }
    let available = if request.keyboard() {
        skylight::keyboard_available()
    } else {
        skylight::pointer_available()
    };
    let mut psn = [0u8; 8];
    if !available || !skylight::get_process_psn_for_window(request.window_id, request.pid, &mut psn)
    {
        return session.finish(
            Err(anyhow::anyhow!(
                "background native prerequisites are unavailable"
            )),
            true,
        );
    }
    let mut scope = skylight::WindowKeyScope::capture(request.pid, request.window_id);
    if !scope.can_restore() {
        return session.finish(
            Err(anyhow::anyhow!(
                "previous key window could not be retained for background cleanup"
            )),
            true,
        );
    }
    let result = (|| {
        if request.keyboard() {
            scope.prime()?;
            control::arm_keyboard(request.window_id);
        } else {
            anyhow::ensure!(
                skylight::activate_without_raise_with(request.pid, request.window_id, false),
                "background focus preparation failed"
            );
            control::sleep(std::time::Duration::from_millis(50));
        }
        control::check()?;
        let local = Some((request.window_x, request.window_y));
        match request.operation.as_str() {
            "click" if request.button == "left" && request.chromium => mouse::click_at_xy_chromium(
                request.pid,
                request.screen_x,
                request.screen_y,
                request.window_x,
                request.window_y,
                request.window_id,
                request.count as usize,
                &[],
            )?,
            "click" if request.button == "left" => mouse::click_at_xy_generic(
                request.pid,
                request.screen_x,
                request.screen_y,
                request.window_x,
                request.window_y,
                request.window_id,
                request.count as usize,
                &[],
            )?,
            "click" => mouse::right_click_at_xy_with_window_local(
                request.pid,
                request.screen_x,
                request.screen_y,
                request.window_x,
                request.window_y,
                request.window_id,
                &[],
            )?,
            "scroll" => {
                // Split large line deltas without truncating requested movement.
                let ticks = request
                    .delta_x
                    .unsigned_abs()
                    .max(request.delta_y.unsigned_abs())
                    .div_ceil(10)
                    .max(1);
                let mut x = request.delta_x;
                let mut y = request.delta_y;
                for _ in 0..ticks {
                    let dx = x.clamp(-10, 10);
                    let dy = y.clamp(-10, 10);
                    mouse::scroll_wheel_at_xy(
                        request.pid,
                        request.screen_x,
                        request.screen_y,
                        local,
                        Some(request.window_id),
                        -dy,
                        -dx,
                        1,
                    )?;
                    x -= dx;
                    y -= dy;
                }
            }
            "type_text" => keyboard::type_text(request.pid, &request.text)?,
            "keypress" => {
                let (key, mut modifiers) = request.key_chord()?;
                if keyboard::is_modifier(key) {
                    modifiers.push(key);
                    keyboard::with_pid_modifier_keys(request.pid, &modifiers, || Ok(()))?;
                } else {
                    keyboard::with_pid_modifier_keys(request.pid, &modifiers, || {
                        keyboard::press_key(request.pid, key, &modifiers)
                    })?;
                }
            }
            _ => unreachable!(),
        }
        control::check()
    })();
    scope.cleanup();
    session.finish(result, false)
}
/// Private foreground bridge. Swift owns target validation, the physical
/// lease, dispatch uncertainty, and restoration around this single exact-window
/// sequence (one front-process request followed by paired make-key records).
#[no_mangle]
pub extern "C" fn dtw_set_front_process(pid: i32, window_id: u32) -> bool {
    pid > 0 && window_id > 0 && skylight::make_exact_window_key(pid, window_id)
}

/// Read-only proof: 1 = current front process, 0 = another process. Negative
/// values retain the native absence reason; only read/identity absence is
/// transient, while missing symbols/invalid PID never become retryable proof.
#[no_mangle]
pub extern "C" fn dtw_front_process_matches(pid: i32) -> i32 {
    match skylight::front_process_matches(pid) {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(reason) => reason as i32,
    }
}

/// The callback context must remain alive until this synchronous call returns.
#[no_mangle]
pub unsafe extern "C" fn dtw_cua_input_call(
    json: *const c_char,
    context: *mut c_void,
    validate: Option<Validate>,
) -> *mut c_char {
    let result=std::panic::catch_unwind(||{
        if json.is_null()||validate.is_none(){return control::rejected("invalid_arguments","missing request or target validator",false);}
        match CStr::from_ptr(json).to_str(){Ok(json)=>call(json,context,validate.unwrap()),Err(error)=>control::rejected("invalid_arguments",&error.to_string(),false)}
    }).unwrap_or_else(|_|serde_json::json!({"ok":false,"dispatch":"unknown","fallback_allowed":false,"error":{"code":"input_panic","message":"native input owner panicked"}}));
    CString::new(result.to_string())
        .expect("JSON has no literal NUL")
        .into_raw()
}
#[no_mangle]
pub unsafe extern "C" fn dtw_cua_input_free(json: *mut c_char) {
    if !json.is_null() {
        drop(CString::from_raw(json));
    }
}
#[no_mangle]
pub extern "C" fn dtw_cua_input_quiesce(timeout_ms: u32) -> bool {
    control::quiesce(timeout_ms)
}
pub fn quiesce(timeout_ms: u32) -> bool {
    control::quiesce(timeout_ms)
}

#[link(name = "objc")]
extern "C" {
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}
struct AutoreleasePool(*mut c_void);
impl AutoreleasePool {
    fn new() -> Self {
        Self(unsafe { objc_autoreleasePoolPush() })
    }
}
impl Drop for AutoreleasePool {
    fn drop(&mut self) {
        unsafe { objc_autoreleasePoolPop(self.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_key_array_is_preserved() {
        let request:Request=serde_json::from_value(serde_json::json!({"operation":"keypress","pid":5,"window_id":7,"deadline_unix_ms":100,"keys":["META","SHIFT","A"]})).unwrap();
        request.validate().unwrap();
        let (key, mods) = request.key_chord().unwrap();
        assert_eq!(key, "A");
        assert_eq!(mods, vec!["META", "SHIFT"]);
    }
    #[test]
    fn public_right_double_click_declines_before_native_preparation() {
        extern "C" fn unexpected(_: *mut c_void, _: i32) -> i32 {
            panic!("unsupported route performed native preparation")
        }
        let result = call(
            r#"{"operation":"click","pid":5,"window_id":7,"deadline_unix_ms":100,"button":"right","count":2}"#,
            std::ptr::null_mut(),
            unexpected,
        );
        assert_eq!(result["dispatch"], "not_dispatched");
        assert_eq!(result["fallback_allowed"], true);
    }
    #[test]
    fn menu_equivalents_and_chromium_supplementary_text_decline_before_native_preparation() {
        extern "C" fn unexpected(_: *mut c_void, _: i32) -> i32 {
            panic!("unsupported route performed native preparation");
        }
        for args in [
            serde_json::json!({"operation":"keypress", "keys":["META", "A"]}),
            serde_json::json!({"chromium":false, "operation":"keypress", "keys":["CMD", "W"]}),
            serde_json::json!({"chromium":false, "operation":"keypress", "keys":["META", "S"]}),
            serde_json::json!({"operation":"keypress", "keys":["command", "SHIFT", "s"]}),
            serde_json::json!({"operation":"keypress", "keys":["WIN", ","]}),
            serde_json::json!({"operation":"type_text", "text":"BMP prefix then 🐦 and suffix"}),
            serde_json::json!({"chromium":false, "operation":"type_text", "text":"prefix\nsecond line\tsuffix"}),
            serde_json::json!({"chromium":false, "operation":"type_text", "text":"prefix\r\nsuffix"}),
            serde_json::json!({"chromium":false, "operation":"keypress", "keys":["RETURN"]}),
            serde_json::json!({"chromium":false, "operation":"keypress", "keys":["SHIFT", "TAB"]}),
        ] {
            let mut request =
                serde_json::json!({"chromium":true,"pid":5,"window_id":7,"deadline_unix_ms":100});
            request
                .as_object_mut()
                .unwrap()
                .extend(args.as_object().unwrap().clone());
            let result = call(&request.to_string(), std::ptr::null_mut(), unexpected);
            assert_eq!(result["dispatch"], "not_dispatched", "{request}");
            assert_eq!(result["fallback_allowed"], true, "{request}");
            assert_eq!(
                result["error"]["code"], "background_unavailable",
                "{request}"
            );
        }
    }
    #[test]
    fn chromium_preflight_keeps_plain_text_and_non_menu_modifiers_on_background_route() {
        for args in [
            serde_json::json!({"operation":"type_text", "text":"后台复核 café"}),
            serde_json::json!({"operation":"keypress", "keys":["A"]}),
            serde_json::json!({"operation":"keypress", "keys":["SHIFT", "A"]}),
            serde_json::json!({"operation":"keypress", "keys":["CTRL", "A"]}),
            serde_json::json!({"operation":"keypress", "keys":["ALT", "LEFT"]}),
            serde_json::json!({"operation":"keypress", "keys":["META", "LEFT"]}),
            serde_json::json!({"operation":"keypress", "keys":["META"]}),
            serde_json::json!({"operation":"keypress", "keys":["BACKSPACE"]}),
        ] {
            let mut request =
                serde_json::json!({"chromium":true,"pid":5,"window_id":7,"deadline_unix_ms":100});
            request
                .as_object_mut()
                .unwrap()
                .extend(args.as_object().unwrap().clone());
            let request: Request = serde_json::from_value(request).unwrap();
            request.validate().unwrap();
            assert_eq!(request.foreground_requirement(), None);
        }
        for args in [
            serde_json::json!({"operation":"type_text", "text":"AppKit 😀"}),
            serde_json::json!({"operation":"keypress", "keys":["CTRL", "A"]}),
        ] {
            let mut request = serde_json::json!({"pid":5,"window_id":7,"deadline_unix_ms":100});
            request
                .as_object_mut()
                .unwrap()
                .extend(args.as_object().unwrap().clone());
            let request: Request = serde_json::from_value(request).unwrap();
            request.validate().unwrap();
            assert_eq!(request.foreground_requirement(), None);
        }
    }
    #[test]
    fn invalid_arguments_never_reach_the_validator() {
        extern "C" fn unexpected(_: *mut c_void, _: i32) -> i32 {
            panic!("invalid input called native validation")
        }
        let result = call(
            r#"{"operation":"keypress","pid":5,"window_id":7,"deadline_unix_ms":100,"keys":["invented"]}"#,
            std::ptr::null_mut(),
            unexpected,
        );
        assert_eq!(result["dispatch"], "not_dispatched");
        assert_eq!(result["fallback_allowed"], false);
    }
}

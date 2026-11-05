//! Minimal CUA native primitives, with DTW-owned identity and execution lifetime.
mod ax;
mod control;
mod keyboard;
mod menu_equivalent;
mod mouse;
mod skylight;
pub(super) use control::pointer_point;
pub use control::Validate;
use serde::Deserialize;
use serde_json::Value;
use std::ffi::{c_char, c_void, CStr, CString};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default)]
    input_framework: String,
    #[serde(default)]
    native_modal: bool,
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
    #[serde(default)]
    path: Vec<[f64; 2]>,
}
fn one() -> u32 {
    1
}
fn left() -> String {
    "left".into()
}
pub(super) fn pointer_available() -> bool {
    skylight::pointer_available()
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
            "move" | "drag" => {
                anyhow::ensure!(
                    if self.operation == "move" {
                        self.path.len() == 1
                    } else {
                        (2..=256).contains(&self.path.len())
                    },
                    "invalid pointer path"
                );
                anyhow::ensure!(
                    self.path.iter().flatten().all(|p| p.is_finite()),
                    "non-finite pointer path"
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
    fn chromium(&self) -> bool {
        self.input_framework == "chromium"
    }
    fn foreground_requirement(&self) -> Option<&'static str> {
        // Chromium's Command chords have an observed renderer-focus boundary.
        // Native menu shortcuts use the CUA menu route below; Control/Option
        // editing chords keep PID delivery rather than being broadly fronted.
        if self.operation == "keypress" && (self.chromium() || self.native_modal) {
            let (key, modifiers) = self.key_chord().ok()?;
            return (!keyboard::is_modifier(key)
                && modifiers
                    .iter()
                    .any(|modifier| matches!(keyboard::key_name_to_code(modifier), Ok(55))))
            .then_some(if self.chromium() {
                "chromium_command_chord"
            } else {
                "native_modal_command_chord"
            });
        }
        if self.operation != "type_text" {
            return None;
        }
        if self.input_framework == "appkit" && self.text.contains(['\n', '\r']) {
            return Some("appkit_multiline_text");
        }
        if self.chromium() && self.text.chars().any(|ch| ch.len_utf16() > 1) {
            return Some("chromium_supplementary_unicode");
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
    if let Some(reason) = request.foreground_requirement() {
        let mut result = control::rejected("background_unavailable", reason, true);
        result["capability_evidence"] = serde_json::json!("native_keyboard_semantics");
        result["capability_detail"] = serde_json::json!(reason);
        return result;
    }
    let _pool = AutoreleasePool::new();
    let session = match control::Session::start(
        request.pid,
        request.deadline_unix_ms,
        context,
        validate,
        !request.chromium(),
    ) {
        Ok(session) => session,
        Err(error) => return error,
    };
    if !request.keyboard() {
        control::set_pointer_point(
            (request.screen_x, request.screen_y),
            (request.window_x, request.window_y),
        );
    }
    if let Err(error) = control::check() {
        return session.finish(Err(error), false);
    }
    // A sheet/dialog receives window-local shortcuts, not its parent
    // document's menu commands. Never dispatch that sibling's menu on its behalf.
    let combo = if request.operation == "keypress" && !request.chromium() && !request.native_modal {
        let (key, modifiers) = request.key_chord().expect("validated chord");
        menu_equivalent::Combo::parse(
            &modifiers.iter().map(|m| m.to_string()).collect::<Vec<_>>(),
            key,
        )
    } else {
        None
    };
    let menu = if let Some(combo) = &combo {
        match menu_equivalent::find(request.pid, request.window_id, combo) {
            Ok(menu_equivalent::Lookup::Match(item)) => Some(item),
            Ok(menu_equivalent::Lookup::NoMatch) => None,
            Ok(menu_equivalent::Lookup::Unavailable(detail)) => {
                // Lookup and destination checks are read-only; no menu level
                // was opened and no key-window preparation has run.
                let mut result = session.finish(Err(anyhow::anyhow!(detail)), true);
                result["capability_evidence"] = serde_json::json!("native_keyboard_semantics");
                result["capability_detail"] = serde_json::json!(detail);
                return result;
            }
            Err(error) => return session.finish(Err(error), false),
        }
    } else {
        None
    };
    let available = if request.keyboard() {
        skylight::keyboard_available()
    } else {
        skylight::pointer_available()
    };
    let mut psn = [0u8; 8];
    if !available {
        let mut result = session.finish(
            Err(anyhow::anyhow!(
                "background native prerequisites are unavailable"
            )),
            true,
        );
        result["capability_evidence"] = serde_json::json!("addressed_event_api_unavailable");
        return result;
    }
    if !skylight::get_process_psn_for_window(request.window_id, request.pid, &mut psn) {
        return session.finish(
            Err(anyhow::anyhow!(
                "Addressed window connection could not be proven"
            )),
            false,
        );
    }
    // Pointer events address their own window and do not borrow a sibling's
    // key window. Requiring AX restoration here blocked sparse-AX apps and
    // turned a successful popup-closing click into an unrelated cleanup error.
    if request.keyboard() && ax::focused_window_absent(request.pid) {
        let mut result = session.finish(
            Err(anyhow::anyhow!(
                "Application has no background key window to retain"
            )),
            true,
        );
        result["capability_evidence"] = serde_json::json!("background_key_window_absent");
        return result;
    }
    let mut scope = request
        .keyboard()
        .then(|| skylight::WindowKeyScope::capture(request.pid, request.window_id));
    if scope.as_ref().is_some_and(|scope| !scope.can_restore()) {
        return session.finish(
            Err(anyhow::anyhow!(
                "previous key window could not be retained for background cleanup"
            )),
            false,
        );
    }
    let result = (|| {
        if request.keyboard() {
            scope.as_mut().unwrap().prime()?;
            control::arm_keyboard(request.window_id);
        } else {
            anyhow::ensure!(
                skylight::activate_without_raise_with(request.pid, request.window_id, false),
                "background focus preparation failed"
            );
            control::sleep(std::time::Duration::from_millis(50));
        }
        control::check()?;
        if let Some(menu) = &menu {
            menu.invoke(request.pid, request.window_id, combo.as_ref().unwrap())?;
            return Ok(());
        }
        let local = Some((request.window_x, request.window_y));
        match request.operation.as_str() {
            "click" if request.button == "left" && request.chromium() => {
                mouse::click_at_xy_chromium(
                    request.pid,
                    request.screen_x,
                    request.screen_y,
                    request.window_x,
                    request.window_y,
                    request.window_id,
                    request.count as usize,
                    &[],
                )?
            }
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
            "click" => mouse::click_at_xy_button(
                request.pid,
                request.screen_x,
                request.screen_y,
                request.window_x,
                request.window_y,
                request.window_id,
                request.count as usize,
                core_graphics::event::CGMouseButton::Right,
                &[],
            )?,
            "move" | "drag" => mouse::pointer_path(
                request.pid,
                request.window_id,
                (
                    request.screen_x - request.window_x,
                    request.screen_y - request.window_y,
                ),
                &request.path,
                request.operation == "drag",
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
        control::check_completed()
    })();
    if let Some(scope) = scope.as_mut() {
        scope.cleanup();
    }
    let mut result = session.finish(result, false);
    if menu.is_some() {
        result["transport"] = serde_json::json!("ax_menu_equivalent");
    }
    result
}
/// Private foreground bridge. Swift owns target validation, the physical
/// lease, dispatch uncertainty, and restoration around this single exact-window
/// sequence (one front-process request followed by paired make-key records).
#[no_mangle]
pub extern "C" fn dtw_set_front_process(pid: i32, window_id: u32) -> bool {
    pid > 0 && window_id > 0 && skylight::make_exact_window_key(pid, window_id)
}

/// Physical pointer input needs app activation, not AX-style synthetic window-key
/// records. Swift retains the same foreground/input lease and readback proof.
#[no_mangle]
pub extern "C" fn dtw_set_visual_front_process(pid: i32, window_id: u32) -> bool {
    pid > 0 && window_id > 0 && skylight::make_process_front(pid, window_id)
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

/// Positive PID only after a stable WindowServer PSN/PID/PSN roundtrip.
/// Negative values preserve a failed identity read; zero is never an app.
#[no_mangle]
pub extern "C" fn dtw_front_process_pid() -> i32 {
    match skylight::front_process_pid() {
        Ok(pid) => pid,
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
    fn shortcut_policy_preserves_literal_and_shift_only_background_input() {
        let make = |operation: &str, framework: &str, keys: &[&str], text: &str| {
            serde_json::from_value::<Request>(serde_json::json!({"operation":operation,
                "input_framework":framework,"pid":5,"window_id":7,"deadline_unix_ms":100,
                "keys":keys,"text":text}))
            .unwrap()
        };
        for framework in ["appkit", "chromium", "unknown"] {
            for keys in [
                &["META", "S"][..],
                &["META", "SHIFT", "T"],
                &["CTRL", "A"],
                &["ALT", "LEFT"],
                &["META", "Z"],
            ] {
                let request = make("keypress", framework, keys, "");
                request.validate().unwrap();
                assert_eq!(
                    request.foreground_requirement(),
                    (framework == "chromium" && keys.contains(&"META"))
                        .then_some("chromium_command_chord")
                );
            }
            for keys in [
                &["A"][..],
                &["LEFT"],
                &["DELETE"],
                &["SHIFT", "A"],
                &["META"],
            ] {
                let request = make("keypress", framework, keys, "");
                request.validate().unwrap();
                assert_eq!(request.foreground_requirement(), None);
            }
            for text in ["hello 123", "普通中文文本框输入"] {
                assert_eq!(
                    make("type_text", framework, &[], text).foreground_requirement(),
                    None
                );
            }
        }
        assert_eq!(
            make("type_text", "appkit", &[], "line\n").foreground_requirement(),
            Some("appkit_multiline_text")
        );
        assert_eq!(
            make("type_text", "chromium", &[], "🌍").foreground_requirement(),
            Some("chromium_supplementary_unicode")
        );
    }
    #[test]
    fn public_key_array_is_preserved() {
        let request:Request=serde_json::from_value(serde_json::json!({"operation":"keypress","pid":5,"window_id":7,"deadline_unix_ms":100,"keys":["META","SHIFT","A"]})).unwrap();
        request.validate().unwrap();
        let (key, mods) = request.key_chord().unwrap();
        assert_eq!(key, "A");
        assert_eq!(mods, vec!["META", "SHIFT"]);
    }
    #[test]
    fn right_double_click_is_an_addressed_pointer_capability() {
        let request:Request=serde_json::from_str(r#"{"operation":"click","pid":5,"window_id":7,"deadline_unix_ms":100,"button":"right","count":2}"#).unwrap();
        request.validate().unwrap();
        assert_eq!(request.foreground_requirement(), None);
    }
    #[test]
    fn shortcut_decline_precedes_native_preparation_and_any_input() {
        extern "C" fn unexpected(_: *mut c_void, _: i32) -> i32 {
            panic!("foreground policy reached native preparation")
        }
        for (framework, args, detail) in [
            (
                "chromium",
                serde_json::json!({"operation":"keypress","keys":["CMD","A"]}),
                "chromium_command_chord",
            ),
            (
                "appkit",
                serde_json::json!({"operation":"keypress","keys":["META","SHIFT","G"],"native_modal":true}),
                "native_modal_command_chord",
            ),
            (
                "appkit",
                serde_json::json!({"operation":"type_text","text":"prefix\nsecond line"}),
                "appkit_multiline_text",
            ),
            (
                "chromium",
                serde_json::json!({"operation":"type_text","text":"prefix 🌍 suffix"}),
                "chromium_supplementary_unicode",
            ),
        ] {
            let mut request = serde_json::json!({"input_framework":framework,"pid":5,"window_id":7,"deadline_unix_ms":100});
            request
                .as_object_mut()
                .unwrap()
                .extend(args.as_object().unwrap().clone());
            let result = call(&request.to_string(), std::ptr::null_mut(), unexpected);
            assert_eq!(result["dispatch"], "not_dispatched", "{result}");
            assert_eq!(result["fallback_allowed"], true, "{result}");
            assert_eq!(result["capability_detail"], detail, "{result}");
            assert!(
                super::super::input_route::fallback_reason(&result).is_some(),
                "{result}"
            );
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

//! Evidence-based recovery hints; never dispatch input or claim AX support.
use crate::SavedResult;
use serde_json::{json, Value};

pub fn matched(value: &Value, expected: &str) -> bool {
    let expected = expected.to_lowercase();
    value["elements"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            // A window title or application menu is not loaded window content.
            let role = item["role"].as_str().unwrap_or("");
            !role.is_empty()
                && !role.starts_with("AXMenu")
                && !role.starts_with("Menu")
                && !matches!(
                    role,
                    "AXWindow"
                        | "AXApplication"
                        | "AXMenuBar"
                        | "AXMenuBarItem"
                        | "Window"
                        | "MenuBar"
                )
                && ["label", "name", "value", "text"].iter().any(|key| {
                    item[*key]
                        .as_str()
                        .is_some_and(|s| s.to_lowercase().contains(&expected))
                })
        })
    })
}

pub fn blocked(value: &Value) -> bool {
    let reason = value["degraded_reason"].as_str().unwrap_or("");
    reason.starts_with("screen_locked") || reason.contains("permission_denied")
}

pub fn annotate(result: &mut SavedResult, field: &str, metadata: Value) {
    let mut value = serde_json::from_str::<Value>(&result.structured).unwrap_or(Value::Null);
    if !value.is_object() {
        value = json!({"native_result":value,"native_text":result.text});
    }
    value[field] = metadata;
    result.structured = value.to_string();
}

pub fn focus(mut result: SavedResult, pid: u32, window_id: Option<u64>) -> SavedResult {
    let value = serde_json::from_str::<Value>(&result.structured).unwrap_or(Value::Null);
    // Exact success requires explicit native evidence, not process activation.
    let verified = !result.is_error
        && if window_id.is_some() {
            value.pointer("/exact_window_effect/verified") == Some(&json!(true))
                || value["landed_on_target"] == true
        } else {
            value["activated"] == true || value["landed_on_target"] == true
        };
    let metadata = if verified {
        json!({"verification":"native_verified","next_call":{"operation":"windows","arguments":{"pid":pid}},
            "hint":"Reobserve the selected window before input; focus alone does not prove application content is ready."})
    } else {
        json!({"verification":"unverified","next_call":{"operation":"windows","arguments":{"pid":pid}},
            "requested_window_id":window_id,
            "hint":"Enumerate again, reconcile the exact window ID, title and visibility, then use observe on that window. A hidden or unavailable window may require user restoration. Do not automatically repeat focus, activate another window, or replay input."})
    };
    annotate(&mut result, "dtw_recovery", metadata);
    result.native_operation = Some("focus".into());
    result
}

/// Native next-call adaptation must not rewrite DTW's own validated recovery.
pub fn presentation(raw: &str, id: &str, discovery: &Value) -> Value {
    let mut value: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
    let metadata = value.as_object_mut().and_then(|o| o.remove("dtw_recovery"));
    let mut shown = crate::native_tools::presentation(&value.to_string(), id, discovery);
    if let Some(metadata) = metadata {
        shown["dtw_recovery"] = metadata;
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    fn saved(value: Value) -> SavedResult {
        SavedResult {
            text: "native focus evidence".into(),
            structured: value.to_string(),
            images: vec![],
            is_error: false,
            error_code: None,
            native_operation: None,
        }
    }
    #[test]
    fn process_activation_is_not_exact_focus_verification() {
        let native = json!({"pid":1,"status":"partial","process_activated":true,"exact_window_effect":{"verified":false},"code":"bring_to_front_exact_window_unverified"});
        let result = focus(saved(native.clone()), 1, Some(2));
        let value: Value = serde_json::from_str(&result.structured).unwrap();
        assert_eq!(value["code"], native["code"]);
        assert_eq!(value["status"], "partial");
        assert_eq!(value["dtw_recovery"]["verification"], "unverified");
        assert_eq!(
            value["dtw_recovery"]["next_call"],
            json!({"operation":"windows","arguments":{"pid":1}})
        );
        assert!(!result.is_error); // Do not reinterpret upstream partial as success/failure.
        let shown = presentation(&result.structured, "receipt", &json!({}));
        assert_eq!(shown["dtw_recovery"], value["dtw_recovery"]);
        assert_eq!(shown["code"], native["code"]);
    }
    #[test]
    fn readiness_never_uses_window_title_or_node_count() {
        assert!(!matched(
            &json!({"element_count":99,"elements":[{"role":"AXWindow","label":"Settings"}]}),
            "Settings"
        ));
        assert!(matched(
            &json!({"elements":[{"role":"AXButton","label":"Settings loaded"}]}),
            "settings"
        ));
        assert!(!matched(&json!({"elements":[]}), "settings"));
        assert!(blocked(
            &json!({"degraded_reason":"screen_locked: wake and unlock"})
        ));
        assert!(!blocked(&json!({"degraded_reason":"ax_app_launching"})));
    }
    #[test]
    fn focus_text_only_errors_keep_original_diagnostics() {
        let mut raw = saved(Value::Null);
        raw.is_error = true;
        raw.error_code = Some("native_error".into());
        let result = focus(raw, 1, Some(2));
        let value: Value = serde_json::from_str(&result.structured).unwrap();
        assert!(result.is_error);
        assert_eq!(result.error_code.as_deref(), Some("native_error"));
        assert_eq!(value["native_text"], "native focus evidence");
    }
}

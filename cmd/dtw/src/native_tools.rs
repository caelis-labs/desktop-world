//! Bounded native routes outside CUA's script allowlist. Authorization and
//! exact browser bindings remain owned by the same CUA session/worker.
use cua_driver_sdk::ToolResult;
use serde_json::{json, Value};

pub const OPERATIONS: &[&str] = &[
    "get_browser_state",
    "browser_prepare",
    "browser_navigate",
    "browser_click",
    "browser_type",
    "browser_select",
    "browser_dialog",
    "browser_pointer",
    "browser_set_input_files",
    "browser_download",
    "health_report",
    "get_guide",
];

pub fn allowed(name: &str) -> bool {
    OPERATIONS.contains(&name)
}

/// Native action refusals are sometimes successful MCP responses. Preserve
/// their complete payload while mapping DTW's outer job status to failed.
pub fn normalize(mut result: ToolResult) -> ToolResult {
    let value = result
        .structured_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    if let Some(value) = value {
        if value["effect"] == "refused" || value["status"] == "refused" {
            result.is_error = true;
            if result.error_code.is_none() {
                result.error_code = value
                    .pointer("/error/code")
                    .or_else(|| value.pointer("/refusal/code"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| Some("native_refused".into()));
            }
        }
    }
    result
}

pub fn arguments(value: Option<&Value>) -> Result<Value, &'static str> {
    let args = value.cloned().unwrap_or_else(|| json!({}));
    let object = args.as_object().ok_or("arguments must be an object")?;
    if object.keys().any(|key| {
        key.starts_with('_') || matches!(key.as_str(), "session" | "session_id" | "public_session")
    }) {
        return Err(
            "session_managed_by_runtime: omit session and private context fields from arguments",
        );
    }
    if args.to_string().len() > 256 * 1024 {
        return Err("native arguments exceed 256 KiB");
    }
    Ok(args)
}

pub fn description(inventory: &Value, name: &str) -> Option<Value> {
    if !allowed(name) {
        return None;
    }
    let tool = inventory
        .get("tools")?
        .as_array()?
        .iter()
        .find(|t| t["name"] == name)?;
    let mut schema = tool.get("inputSchema")?.clone();
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.retain(|key, _| !key.starts_with('_') && key != "session");
    }
    if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|key| key != "session");
    }
    Some(json!({
        "operation":name,"description":tool["description"],"arguments_schema":schema,
        "annotations":tool["annotations"],
        "usage":"Call exec with this operation and native arguments nested under arguments. The runtime supplies the session. Preserve target_id/tab_id/refs exactly; no automatic setup, foreground fallback or replay."
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_native_action_is_not_a_completed_job() {
        for value in [
            json!({"effect":"refused","error":{"code":"browser_ref_stale"}}),
            json!({"status":"refused","refusal":{"code":"browser_ref_stale"}}),
        ] {
            let raw = value.to_string();
            let result = normalize(ToolResult {
                text: "refused".into(),
                structured_json: Some(raw.clone()),
                images: vec![],
                action: None,
                verification: None,
                degraded: false,
                raw_json: String::new(),
                is_error: false,
                error_code: None,
            });
            assert!(result.is_error);
            assert_eq!(result.error_code.as_deref(), Some("browser_ref_stale"));
            assert_eq!(result.structured_json.as_deref(), Some(raw.as_str()));
        }
        let result = normalize(ToolResult {
            text: "degraded health".into(),
            structured_json: Some(
                json!({"status":"degraded","checks":[{"status":"failed"}]}).to_string(),
            ),
            images: vec![],
            action: None,
            verification: None,
            degraded: false,
            raw_json: String::new(),
            is_error: false,
            error_code: None,
        });
        assert!(!result.is_error);
    }

    #[test]
    fn browser_arguments_cannot_override_runtime_authority() {
        for key in [
            "session",
            "session_id",
            "public_session",
            "_session_id",
            "_permission_mode",
            "_authorization_context",
            "_cua_browser_download_mcp_host_approved",
        ] {
            assert!(arguments(Some(&json!({key:"forged"}))).is_err());
        }
        assert!(arguments(Some(&json!([]))).is_err());
        let args = json!({"target_id":"opaque", "tab_id":"tab", "text":"_session_id is ordinary input text"});
        assert_eq!(arguments(Some(&args)).unwrap(), args);
        for name in [
            "run_script",
            "page",
            "kill_app",
            "set_config",
            "replay_trajectory",
        ] {
            assert!(!allowed(name));
        }
    }

    #[test]
    fn schemas_come_from_worker_without_session_parameter() {
        let inventory = json!({"tools":[{"name":"browser_click","description":"native",
            "inputSchema":{"type":"object","properties":{"session":{},"target_id":{"type":"string"}},"required":["session","target_id"]}}]});
        let described = description(&inventory, "browser_click").unwrap();
        assert_eq!(
            described["arguments_schema"]["required"],
            json!(["target_id"])
        );
        assert!(described["arguments_schema"]["properties"]
            .get("session")
            .is_none());
        assert!(description(&inventory, "page").is_none());
    }
}

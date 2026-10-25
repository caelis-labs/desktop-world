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
    if name == "browser_download" {
        schema["properties"]["destination_root"]["description"] = json!("Existing absolute directory selected for this task. Ordinary OS paths are accepted; DTW canonicalizes them for CUA.");
    }
    if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|key| key != "session");
    }
    Some(json!({
        "operation":name,"description":description_text(name),"arguments_schema":schema,
        "annotations":tool["annotations"],
        "usage":"Call exec with this operation and native arguments nested under arguments. The runtime supplies the session. Preserve target_id/tab_id/refs exactly; no automatic setup, foreground fallback or replay."
    }))
}

fn description_text(name: &str) -> &'static str {
    match name {
        "get_browser_state" => "Bind an exact PID/window, or observe a returned target_id/tab_id. The normal receipt includes tabs, page structure, refs and continuation. New observations invalidate old refs.",
        "browser_prepare" => "Prepare a browser. Use a target from describe for a configured existing profile. For a task-owned isolated browser use allow_launch:true and profile:{mode:isolated_new}. DTW supplies session. Existing-profile grants are configured by the Harness; a refusal does not authorize fallback or relaunch.",
        "browser_navigate" => "Navigate the exact target/tab, then observe it for fresh refs.",
        "browser_click" => "Click a live ref or viewport CSS point on the exact target/tab. Observe the effect; never replay automatically.",
        "browser_type" => "Type into a live browser ref; text and optional replace follow the native schema.",
        "browser_select" => "Select an exact value or label on a live select ref, then verify.",
        "browser_pointer" => "Hover, right-click, double-click, scroll or drag using live browser refs or CSS points. Delivery and actual page effect are distinct.",
        "browser_dialog" => "Inspect a JavaScript dialog, then accept or dismiss its exact dialog_id. Inspect before a known synchronous trigger; a trigger timeout does not mean it was not delivered.",
        "browser_set_input_files" => "Assign task-authorized regular files to a live file-input ref; native file scope still applies.",
        "browser_download" => "Download once through a live ref into an existing absolute destination_root. The Harness authorizes this tool call; DTW normalizes OS path spelling and CUA checks the live binding and file scope. No extra DTW form or approval argument is needed.",
        "health_report" => "Return native runtime diagnostics; individual checks may be degraded without the call failing.",
        "get_guide" => "Read an upstream diagnostic reference. For callable DTW operations and recovery use describe; upstream-only tools and startup flags are not DTW instructions.",
        _ => "Native DTW operation; arguments follow the current worker schema.",
    }
}

pub fn available(discovery: &Value, operation: &str) -> bool {
    discovery["operations"]
        .as_array()
        .is_some_and(|ops| ops.contains(&json!(operation)))
}

/// Adapt instructions, not native evidence. The exact original remains in the
/// structured result page. Never ask the model to translate a native session.
fn adapt(value: &mut Value, discovery: &Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(|item| adapt(item, discovery)),
        Value::Object(object) => {
            if object
                .get("message")
                .and_then(Value::as_str)
                .is_some_and(|s| {
                    s.contains("--grant") || s.contains("embedding authorization host")
                })
            {
                object.insert("message".into(), json!("Existing-profile access requires Harness configuration. Inspect describe for configured targets; tool-call approval alone does not grant profile access."));
                object.insert("requires".into(), json!("host"));
            }
            if let Some(call) = object.remove("next_call") {
                let name = call["tool"].as_str().unwrap_or("");
                let mut args = call["arguments"].clone();
                if let Some(args) = args.as_object_mut() {
                    args.retain(|k, _| {
                        !k.starts_with('_')
                            && !matches!(k.as_str(), "session" | "session_id" | "public_session")
                    });
                }
                let profile_needs_host = name == "browser_prepare"
                    && args.pointer("/strategy/kind").and_then(Value::as_str)
                        == Some("existing_profile")
                    && discovery["mode"] != "bounded";
                if allowed(name) && available(discovery, name) && !profile_needs_host {
                    object.insert(
                        "next_call".into(),
                        json!({"operation":name,"arguments":args}),
                    );
                    if object.contains_key("hint") {
                        object.insert("hint".into(), json!("Use next_call with exec after reconciling the current UI; it is never run automatically."));
                    }
                } else {
                    object.insert("next_call".into(), json!({"operation":"describe"}));
                    object.insert("hint".into(), json!(if profile_needs_host {
                        "Existing-profile access needs Harness configuration. Use describe to inspect configured targets; do not grant it through tool arguments."
                    } else { "Use describe for available DTW operations. This native recovery requires a new observation or Harness configuration." }));
                    if profile_needs_host {
                        object.insert("requires".into(), json!("host"));
                    }
                }
            }
            for child in object.values_mut() {
                adapt(child, discovery);
            }
        }
        _ => {}
    }
}

pub fn presentation(raw: &str, id: &str, discovery: &Value) -> Value {
    const LIMIT: usize = 10 * 1024;
    let mut value = serde_json::from_str::<Value>(raw).unwrap_or(Value::Null);
    adapt(&mut value, discovery);
    if value.to_string().len() <= LIMIT {
        return value;
    }
    // Compact large observations without chopping an opaque ID or JSON record.
    let more = json!({"operation":"result","id":id,"part":"structured"});
    if let Some(object) = value.as_object_mut() {
        object.insert("more".into(), more.clone());
        object.insert("dtw_truncated".into(), json!(true));
    }
    for field in ["outline", "text", "tree_markdown", "markdown"] {
        if let Some(text) = value.get(field).and_then(Value::as_str).map(str::to_owned) {
            if text.len() > 2048 {
                value[field] = json!(format!("{}…", text.chars().take(512).collect::<String>()));
            }
        }
    }
    for field in ["content_refs", "refs", "tabs"] {
        while value.to_string().len() > LIMIT {
            let Some(items) = value
                .get_mut(field)
                .and_then(Value::as_array_mut)
                .filter(|a| !a.is_empty())
            else {
                break;
            };
            items.pop();
        }
    }
    if value.to_string().len() > LIMIT {
        // Unusually large native diagnostics stay in their original page.
        return json!({"dtw_truncated":true,"more":more});
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_observations_and_recovery_are_directly_actionable() {
        let discovery =
            json!({"mode":"bounded","operations":["browser_prepare","get_browser_state"]});
        let raw = json!({"target_id":"target","tabs":[{"tab_id":"tab"}],"refs":[{"ref":"r1","name":"Apply"}],
            "snapshot":{"continuation":"cursor"},"detail":{"next_call":{"tool":"browser_prepare","arguments":{"session":"private","pid":42,"window_id":7,"strategy":{"kind":"existing_profile"}}}}});
        let shown = presentation(&raw.to_string(), "job", &discovery);
        assert_eq!(shown["tabs"], raw["tabs"]);
        assert_eq!(shown["refs"], raw["refs"]);
        assert_eq!(shown["snapshot"], raw["snapshot"]);
        let call = &shown["detail"]["next_call"];
        assert_eq!(call["operation"], "browser_prepare");
        assert!(arguments(call.get("arguments")).is_ok());
        assert!(call["arguments"].get("session").is_none());
        let standard = presentation(
            &raw.to_string(),
            "job",
            &json!({"mode":"standard","operations":["browser_prepare"]}),
        );
        assert_eq!(standard["detail"]["requires"], "host");
    }

    #[test]
    fn large_observations_keep_whole_refs_and_explicit_pagination() {
        let raw = json!({"target_id":"target","tab_id":"tab", "outline":"x".repeat(30000),
            "refs":(0..1000).map(|n| json!({"ref":format!("exact-ref-{n}"),"name":"button"})).collect::<Vec<_>>(),
            "snapshot":{"continuation":"native-cursor"}});
        let shown = presentation(&raw.to_string(), "abcd", &json!({}));
        assert!(shown.to_string().len() <= 10 * 1024);
        assert_eq!(shown["target_id"], "target");
        assert_eq!(shown["snapshot"]["continuation"], "native-cursor");
        assert_eq!(
            shown["more"],
            json!({"operation":"result","id":"abcd","part":"structured"})
        );
        assert_eq!(shown["refs"][0]["ref"], "exact-ref-0");
        assert!(raw["refs"].as_array().unwrap().len() > shown["refs"].as_array().unwrap().len());
    }

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

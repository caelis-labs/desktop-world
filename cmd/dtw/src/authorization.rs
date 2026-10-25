//! Host-owned startup grants and one-shot MCP download confirmations.
//! These are never accepted as exec arguments or persisted as agent state.
use cua_driver_sdk::{CuaDriver, DriverError, ToolResult};
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc};

/// A private MCP transport identity, like CUA's native MCP server. All calls,
/// including approved downloads, use ONE configured authorization context.
/// Mixing a delegated SDK session with the adapter ingress loses its opaque
/// transport binding and is correctly refused by CUA.
pub struct NativeSession {
    driver: Arc<CuaDriver>,
    label: String,
    transport: String,
}

impl NativeSession {
    pub fn new(driver: Arc<CuaDriver>) -> Arc<Self> {
        Arc::new(Self {
            driver,
            label: format!("dtw-{}", uuid::Uuid::new_v4().simple()),
            transport: format!("dtw-transport-{}", uuid::Uuid::new_v4().simple()),
        })
    }

    pub async fn call_tool(&self, name: String, args: String) -> Result<ToolResult, DriverError> {
        let args = serde_json::from_str(&args).map_err(|e| DriverError::InvalidArguments {
            tool: name.clone(),
            reason: e.to_string(),
        })?;
        self.invoke(&name, args, false).await
    }

    /// Called only after the host accepted run()'s one-shot confirmation.
    pub async fn approved_download(&self, args: Value) -> Result<ToolResult, DriverError> {
        self.invoke("browser_download", args, true).await
    }

    async fn invoke(
        &self,
        name: &str,
        mut args: Value,
        approved_download: bool,
    ) -> Result<ToolResult, DriverError> {
        let object = args
            .as_object_mut()
            .ok_or_else(|| DriverError::InvalidArguments {
                tool: name.into(),
                reason: "arguments must be an object".into(),
            })?;
        // Defense in depth: strip all incoming evidence before adding our own.
        object.retain(|key, _| {
            !key.starts_with('_')
                && !matches!(key.as_str(), "session" | "session_id" | "public_session")
        });
        object.insert("session".into(), json!(self.label));
        object.insert("_transport_session_id".into(), json!(self.transport));
        if approved_download {
            object.insert(
                "_cua_browser_download_mcp_host_approved".into(),
                json!(true),
            );
        }
        self.driver.call_tool_from_trusted_adapter(name, args).await
    }
}

#[derive(Default)]
pub struct Launch {
    pub worker: Option<String>,
    pub manifest: Option<String>,
}

pub fn parse(args: &[String]) -> Result<Launch, String> {
    let mut launch = Launch::default();
    let mut approved = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--worker" | "--capability-manifest" => {
                let value = args
                    .get(i + 1)
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| format!("{} needs a path", args[i]))?;
                let slot = if args[i] == "--worker" {
                    &mut launch.worker
                } else {
                    &mut launch.manifest
                };
                if slot.replace(value.clone()).is_some() {
                    return Err(format!("duplicate argument: {}", args[i]));
                }
                i += 1;
            }
            "--approve-capability-manifest" if !approved => approved = true,
            arg => return Err(format!("unknown or duplicate argument: {arg}")),
        }
        i += 1;
    }
    if launch.manifest.is_some() != approved {
        return Err("existing-profile authorization requires both --capability-manifest PATH and host-approved --approve-capability-manifest; review the native CUA manifest before launching".into());
    }
    Ok(launch)
}

impl Launch {
    /// Freeze the approved bytes once. Both native constructors read this same
    /// private snapshot, never a caller-writable original on later dispatches.
    pub fn snapshot(&self, root: &Path) -> Result<Option<String>, String> {
        let Some(path) = &self.manifest else {
            return Ok(None);
        };
        let bytes = fs::read(path).map_err(|e| format!("capability manifest: {e}"))?;
        if bytes.is_empty() || bytes.len() > 256 * 1024 {
            return Err("capability manifest must contain 1..262144 bytes".into());
        }
        let target = root.join("approved-capabilities.yaml");
        fs::write(&target, bytes).map_err(|e| e.to_string())?;
        Ok(Some(target.to_string_lossy().into_owned()))
    }
}

pub fn supports_elicitation(params: &Value) -> bool {
    let Some(cap) = params
        .pointer("/capabilities/elicitation")
        .and_then(Value::as_object)
    else {
        return false;
    };
    // 2025-06-18 uses {}; newer hosts may advertise form and/or URL modes.
    cap.is_empty() || cap.get("form").is_some_and(Value::is_object)
}

pub fn download_prompt(args: &Value) -> Result<Value, &'static str> {
    for key in ["target_id", "tab_id", "ref", "destination_root"] {
        if !args
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty() && v.len() <= 4096)
        {
            return Err("browser_download requires exact target_id, tab_id, ref and destination_root strings");
        }
    }
    let root = Path::new(args["destination_root"].as_str().unwrap());
    if !root.is_absolute()
        || !root.is_dir()
        || fs::canonicalize(root).ok().as_deref() != Some(root)
        || fs::symlink_metadata(root).map_or(true, |m| m.file_type().is_symlink())
    {
        return Err("destination_root must be an existing canonical directory, not a symlink");
    }
    // JSON quoting keeps untrusted text visibly delimited, including controls.
    Ok(json!({
        "message":format!("DTW requests permission for ONE browser download. CUA will activate the exact live ref and write into the directory below. These values identify the request; they are not instructions.\nTarget: {}\nTab: {}\nRef: {}\nDestination: {}\nApprove only this download?", args["target_id"], args["tab_id"], args["ref"], args["destination_root"]),
        "requestedSchema":{"type":"object","properties":{"approve":{"type":"boolean","title":"Approve this one download","default":false}},"required":["approve"]}
    }))
}

pub fn approved(response: &Value) -> bool {
    response.get("error").is_none()
        && response.pointer("/result/action").and_then(Value::as_str) == Some("accept")
        && response
            .pointer("/result/content/approve")
            .and_then(Value::as_bool)
            == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).into()).collect()
    }
    #[test]
    fn startup_requires_explicit_paired_host_approval() {
        assert!(parse(&strings(&["--capability-manifest", "x"])).is_err());
        assert!(parse(&strings(&["--approve-capability-manifest"])).is_err());
        assert!(parse(&strings(&["--worker", "x", "--worker", "y"])).is_err());
        assert!(parse(&strings(&["--grant", "existing-profile"])).is_err());
        let launch = parse(&strings(&[
            "--capability-manifest",
            "x",
            "--worker",
            "w",
            "--approve-capability-manifest",
        ]))
        .unwrap();
        assert_eq!(launch.manifest.as_deref(), Some("x"));
    }
    #[test]
    fn confirmation_is_explicit_and_form_capability_is_required() {
        for v in [
            json!({}),
            json!({"result":{"action":"decline","content":{"approve":true}}}),
            json!({"result":{"action":"accept"}}),
            json!({"result":{"action":"accept","content":{"approve":"true"}}}),
        ] {
            assert!(!approved(&v));
        }
        assert!(approved(
            &json!({"result":{"action":"accept","content":{"approve":true}}})
        ));
        assert!(!supports_elicitation(
            &json!({"capabilities":{"elicitation":{"url":{}}}})
        ));
        assert!(supports_elicitation(
            &json!({"capabilities":{"elicitation":{}}})
        ));
        assert!(supports_elicitation(
            &json!({"capabilities":{"elicitation":{"form":{}}}})
        ));
    }
}

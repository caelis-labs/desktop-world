//! Host-owned startup scope and the native MCP adapter boundary.
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

    /// The Harness authorizes tools/call, as in CUA's native MCP server.
    /// No model argument or second DTW confirmation grants permission.
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

/// Accept ordinary absolute OS paths; only the native output spelling changes.
/// Never create a directory or silently follow a final-component symlink.
pub fn normalize_download(args: &mut Value) -> Result<(), String> {
    let raw = args
        .get("destination_root")
        .and_then(Value::as_str)
        .ok_or("browser_download requires destination_root")?;
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err("destination_root must be absolute".into());
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("destination_root: {e}"))?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("destination_root must be an existing directory, not a symlink".into());
    }
    args["destination_root"] = json!(fs::canonicalize(path).map_err(|e| e.to_string())?);
    Ok(())
}

/// Presentation only. CUA has already validated and loaded this exact snapshot;
/// it remains the authority for tool/resource checks and grant expiry.
pub fn discovery(manifest: Option<&str>) -> Result<Value, String> {
    let Some(path) = manifest else {
        let mut info = json!({"mode":"standard", "targets":[],
            "operations":["run","app","click","desktop","focus","menu","cursor","permissions",
                "get_browser_state","browser_prepare","browser_navigate","browser_click","browser_type",
                "browser_select","browser_pointer","browser_dialog","browser_set_input_files","browser_download",
                "health_report","get_guide","describe","result","status","cancel"],
            "next_call":{"operation":"run","script":"return await cua.listApps();"},
            "guidance":"Discover the task's application, then observe its window. Existing-profile access requires Harness configuration; an explicitly requested isolated browser needs no profile manifest."});
        if !cfg!(target_os = "macos") {
            info["operations"]
                .as_array_mut()
                .unwrap()
                .retain(|op| !matches!(op.as_str(), Some("app" | "click" | "menu")));
        }
        return Ok(info);
    };
    let raw: Value = serde_yaml_ng::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let allowed = raw
        .pointer("/allow/tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let denied = raw
        .pointer("/deny/tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut operations = vec![
        json!("describe"),
        json!("result"),
        json!("status"),
        json!("cancel"),
    ];
    for (name, native) in crate::native_tools::OPERATIONS
        .iter()
        .map(|name| (*name, *name))
        .chain([
            ("run", "run_script"),
            ("focus", "bring_to_front"),
            ("menu", "invoke_menu"),
            ("desktop", "get_desktop_state"),
            ("permissions", "check_permissions"),
        ])
    {
        if allowed.contains(&json!(native)) && !denied.contains(&json!(native)) {
            operations.push(json!(name));
        }
    }
    let targets = raw
        .pointer("/resources/browser/existing_profiles")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut target| {
            target["next_call"] = json!({"operation":"get_browser_state","arguments":{
                "pid":target["pid"],"window_id":target["window_id"]}});
            target
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"mode":"bounded", "operations":operations, "targets":targets,
        "origins":raw.pointer("/resources/browser/origins"),
        "guidance":"Use a configured target's next_call. These are launch-approved bindings, not a fresh grant: CUA checks live identity and expiry on every call. Do not fall back to scripts, AX or SCK unless listed as available."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).into()).collect()
    }
    #[test]
    fn discovery_excludes_platform_specific_extensions() {
        let info = discovery(None).unwrap();
        assert_eq!(
            crate::native_tools::available(&info, "app"),
            cfg!(target_os = "macos")
        );
        assert!(crate::native_tools::available(&info, "get_browser_state"));
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
    fn download_accepts_normal_paths_without_a_second_permission_model() {
        let root = std::env::temp_dir().join(format!("dtw-path-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let mut args = json!({"destination_root":root.join(".")});
        normalize_download(&mut args).unwrap();
        assert_eq!(
            args["destination_root"],
            json!(fs::canonicalize(&root).unwrap())
        );
        assert!(normalize_download(&mut json!({"destination_root":"relative"})).is_err());
        fs::remove_dir(root).unwrap();
    }
}

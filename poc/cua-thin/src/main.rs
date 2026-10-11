use cua_driver_sdk::{
    ConfiguredDriverOptions, CuaDriver, CuaDriverSession, DriverError, EmbeddedEnvironmentVariable,
    PrivateWorkerOptions, RuntimeAuthorizationOptions, SessionPermissionMode, ToolResult,
    TrustedSessionOptions,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Mutex as AsyncMutex,
};

const MAX_STATE_BYTES: usize = 32 * 1024;
const MODEL_TEXT_BYTES: usize = 12 * 1024;
const PAGE_BYTES: usize = 32 * 1024;

struct Session {
    native: Arc<CuaDriverSession>,
    state: String,
    busy: bool,
    started: bool,
}

struct Job {
    session: String,
    phase: String,
    cancel_requested: bool,
    result: Option<SavedResult>,
}

struct SavedResult {
    text: String,
    structured: String,
    images: Vec<cua_driver_sdk::ImageContent>,
    is_error: bool,
    error_code: Option<String>,
}

enum Work {
    Script(String),
    Cursor(Option<bool>),
    Focus {
        pid: u32,
        window_id: Option<u64>,
    },
    Menu {
        pid: u32,
        window_id: u64,
        path: Vec<String>,
    },
    Desktop {
        max_image_dimension: u32,
    },
}

struct Server {
    driver: Arc<CuaDriver>,
    sessions: Mutex<HashMap<String, Session>>,
    jobs: Mutex<HashMap<String, Job>>,
    stdout: AsyncMutex<io::Stdout>,
}

fn worker_path() -> Result<String, String> {
    if let Some(arg) = env::args().skip(1).next() {
        if arg == "--version" || arg == "version" {
            println!("dtw-cua-poc 0.0.1 (CUA source ac967b9006aa)");
            std::process::exit(0);
        }
        if arg == "--worker" {
            return env::args()
                .nth(2)
                .ok_or_else(|| "--worker needs a binary path".to_owned());
        }
        if arg == "install" {
            let args: Vec<String> = env::args().collect();
            if args.len() != 5 || args[2] != "antigravity" || args[3] != "--project" {
                return Err("usage: dtw install antigravity --project PROJECT_DIR".into());
            }
            install_antigravity(Path::new(&args[4]))?;
            std::process::exit(0);
        }
        return Err(format!("unknown argument: {arg}"));
    }
    if let Ok(path) = env::var("DTW_CUA_WORKER") {
        return Ok(path);
    }
    let binary = env::current_exe().map_err(|e| e.to_string())?;
    let path = binary
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("libexec/cua-driver"))
        .ok_or_else(|| "cannot locate sibling libexec/cua-driver".to_owned())?;
    Ok(path.to_string_lossy().into_owned())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination).map_err(|e| e.to_string())?;
    for item in fs::read_dir(source).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let kind = item.file_type().map_err(|e| e.to_string())?;
        let target = destination.join(item.file_name());
        if kind.is_dir() {
            copy_tree(&item.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(item.path(), &target).map_err(|e| e.to_string())?;
        } else {
            return Err(format!(
                "unsupported package entry: {}",
                item.path().display()
            ));
        }
    }
    Ok(())
}

fn install_antigravity(project: &Path) -> Result<(), String> {
    let project = project.canonicalize().map_err(|e| e.to_string())?;
    if !project.is_dir() {
        return Err("project must be an existing directory".into());
    }
    let binary = env::current_exe().map_err(|e| e.to_string())?;
    let package = binary
        .parent()
        .and_then(|p| p.parent())
        .ok_or("cannot locate package root")?;
    for required in [
        "libexec/cua-driver",
        "skills/desktop-world/SKILL.md",
        "mcp.json",
    ] {
        if !package.join(required).is_file() {
            return Err(format!("incomplete package: {required}"));
        }
    }
    let destination = project.join(".agents/plugins/desktop-world");
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(format!(
            "destination already exists: {}",
            destination.display()
        ));
    }
    fs::create_dir_all(destination.parent().ok_or("invalid destination")?)
        .map_err(|e| e.to_string())?;
    let staging =
        destination.with_file_name(format!(".desktop-world-install-{}", uuid::Uuid::new_v4()));
    let plugin = json!({
        "$schema":"https://antigravity.google/schemas/v1/plugin.json",
        "name":"desktop-world",
        "description":"DTW thin CUA kernel POC"
    });
    let result = (|| {
        copy_tree(package, &staging)?;
        fs::write(staging.join("plugin.json"), format!("{plugin}\n")).map_err(|e| e.to_string())?;
        let mcp = json!({"mcpServers":{"dtw":{"command":destination.join("bin/dtw")}}});
        fs::write(staging.join("mcp_config.json"), format!("{mcp}\n"))
            .map_err(|e| e.to_string())?;
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(format!(
                "destination already exists: {}",
                destination.display()
            ));
        }
        fs::rename(&staging, &destination).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    println!(
        "{}",
        json!({"plugin":destination,"mcp_command":destination.join("bin/dtw")})
    );
    Ok(())
}

fn configured_driver() -> ConfiguredDriverOptions {
    ConfiguredDriverOptions {
        claude_code_compatibility: false,
        authorization: RuntimeAuthorizationOptions {
            allowed_modes: vec![SessionPermissionMode::Standard],
            compatibility_mode: SessionPermissionMode::Standard,
            compatibility_capability_manifest_path: None,
            compatibility_bounded_manifest_path: None,
            unrestricted_acknowledged: false,
            max_session_ttl_seconds: 3600,
            max_idle_ttl_seconds: 1800,
        },
    }
}

fn new_driver(path: String) -> Result<Arc<CuaDriver>, String> {
    if !PathBuf::from(&path).is_file() {
        return Err(format!("private CUA worker missing: {path}"));
    }
    CuaDriver::create_private_worker(PrivateWorkerOptions {
        binary_path: path,
        host_bundle_id: "dev.caelis.dtw.cua-poc".into(),
        startup_timeout_ms: Some(15_000),
        shutdown_timeout_ms: Some(3_000),
        configured_driver: configured_driver(),
        environment: Vec::<EmbeddedEnvironmentVariable>::new(),
        inherit_stderr: true,
    })
    .map_err(|e| e.to_string())
}

fn reply(id: &Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "result":result})
}

fn error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code, "message":message.into()}})
}

fn mcp_text(value: Value, is_error: bool) -> Value {
    json!({"content":[{"type":"text","text":value.to_string()}], "isError":is_error})
}

fn short(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_owned(), false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_owned(), true)
}

fn compact_success_text(value: &str) -> Option<String> {
    if !value.starts_with("run_script: ok") || !value.contains("\nreturn value:") {
        return None;
    }
    let first = value.lines().next()?;
    let calls = value
        .find("\ncalls:\n")
        .map(|at| &value[at..])
        .unwrap_or("");
    Some(format!("{first}{calls}"))
}

fn page(s: &str, offset: usize) -> Value {
    let offset = offset.min(s.len());
    let mut start = offset;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (start + PAGE_BYTES).min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    json!({"data": &s[start..end], "offset":start, "next_offset": if end < s.len() {Some(end)} else {None}, "bytes":s.len()})
}

impl Server {
    async fn write(&self, value: Value) {
        let mut stdout = self.stdout.lock().await;
        let _ = stdout.write_all(value.to_string().as_bytes()).await;
        let _ = stdout.write_all(b"\n").await;
        let _ = stdout.flush().await;
    }

    async fn handle(self: Arc<Self>, request: Value) {
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let Some(id) = request.get("id") else {
            return;
        };
        let response = match method {
            "initialize" => reply(
                id,
                json!({
                    "protocolVersion":"2025-06-18",
                    "capabilities":{"tools":{}},
                    "serverInfo":{"name":"dtw", "version":"0.0.1-cua-poc"}
                }),
            ),
            "ping" => reply(id, json!({})),
            "tools/list" => reply(
                id,
                json!({"tools":[{
                    "name":"exec",
                    "description":"Run CUA's native QuickJS script, or call its exact-window menu and full-desktop snapshot primitives through the same receipt channel. Use the short Skill and api.d.ts for signatures. Fetch retained images with operation=result,part=image.",
                    "inputSchema":{
                        "type":"object",
                        "properties":{
                            "session":{"type":"string","description":"Session label (default: default). POC limitation: native element tokens can cross labels in one MCP process; use a separate process per independent Agent."},
                            "script":{"type":"string","description":"Async CUA run_script body. Use cua.getApp, getState, query, click, typeText, pressKey, zoom and return; see packaged api.d.ts."},
                            "operation":{"type":"string","enum":["run","status","result","cancel","permissions","cursor","focus","menu","desktop"]},
                            "wait_ms":{"type":"integer","minimum":0,"maximum":5000,"description":"Wait briefly for a terminal receipt in the same exec call (default 4000 ms). If still running, query the original execution_id with operation=result."},
                            "include_image":{"type":"boolean","description":"If this execution completes within wait_ms and has an image, return the first image with its summary. Default false; later result,part=image still works."},
                            "enabled":{"type":"boolean","description":"With operation=cursor, show or hide this Session's virtual cursor. Omit to read cursor state."},
                            "pid":{"type":"integer","minimum":1,"description":"With focus/menu, exact app PID."},
                            "window_id":{"type":"integer","minimum":1,"description":"With menu, exact target window; with focus, optional ordinary window ID."},
                            "path":{"type":"array","minItems":1,"maxItems":16,"items":{"type":"string"},"description":"With menu, exact macOS menu labels from menu bar to command."},
                            "max_image_dimension":{"type":"integer","minimum":0,"description":"With desktop, optional screenshot long-edge cap (default 1600, 0 for original size); image is retained for result,part=image."},
                            "execution_id":{"type":"string"},
                            "part":{"type":"string","enum":["summary","text","structured","image"]},
                            "offset":{"type":"integer","minimum":0},
                            "image_index":{"type":"integer","minimum":0}
                        },
                        "additionalProperties":false
                    }
                }]}),
            ),
            "tools/call" => {
                let params = request.get("params").cloned().unwrap_or(Value::Null);
                if params.get("name").and_then(Value::as_str) != Some("exec") {
                    error(id, -32602, "only exec is available")
                } else {
                    let args = params
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    reply(id, self.exec(args).await)
                }
            }
            _ => error(id, -32601, format!("unknown method: {method}")),
        };
        self.write(response).await;
    }

    async fn exec(self: &Arc<Self>, args: Value) -> Value {
        let Some(obj) = args.as_object() else {
            return mcp_text(json!({"error":"arguments must be an object"}), true);
        };
        let session_id = obj
            .get("session")
            .and_then(Value::as_str)
            .unwrap_or("default");
        if session_id.is_empty() || session_id.len() > 100 || !session_id.is_ascii() {
            return mcp_text(json!({"error":"invalid session"}), true);
        }
        let operation = obj
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("run");
        let wait_ms = obj
            .get("wait_ms")
            .and_then(Value::as_u64)
            .unwrap_or(4_000)
            .min(5_000);
        let include_image = obj
            .get("include_image")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match operation {
            "run" => match obj.get("script").and_then(Value::as_str) {
                Some(script) => {
                    self.run(
                        session_id,
                        Work::Script(script.to_owned()),
                        wait_ms,
                        include_image,
                    )
                    .await
                }
                None => mcp_text(json!({"error":"script is required"}), true),
            },
            "cursor" => {
                self.run(
                    session_id,
                    Work::Cursor(obj.get("enabled").and_then(Value::as_bool)),
                    wait_ms,
                    include_image,
                )
                .await
            }
            "focus" => {
                let Some(pid) = obj
                    .get("pid")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0)
                else {
                    return mcp_text(json!({"error":"focus requires positive pid"}), true);
                };
                let window_id = obj.get("window_id").and_then(Value::as_u64);
                if obj.get("window_id").is_some() && window_id.is_none_or(|n| n == 0) {
                    return mcp_text(json!({"error":"window_id must be positive"}), true);
                }
                self.run(
                    session_id,
                    Work::Focus { pid, window_id },
                    wait_ms,
                    include_image,
                )
                .await
            }
            "menu" => {
                let Some(pid) = obj
                    .get("pid")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0)
                else {
                    return mcp_text(json!({"error":"menu requires positive pid"}), true);
                };
                let Some(window_id) = obj
                    .get("window_id")
                    .and_then(Value::as_u64)
                    .filter(|n| *n > 0)
                else {
                    return mcp_text(json!({"error":"menu requires positive window_id"}), true);
                };
                let Some(path) = obj.get("path").and_then(Value::as_array) else {
                    return mcp_text(json!({"error":"menu requires path"}), true);
                };
                if path.is_empty()
                    || path.len() > 16
                    || path
                        .iter()
                        .any(|s| s.as_str().is_none_or(|s| s.trim().is_empty()))
                {
                    return mcp_text(
                        json!({"error":"menu path must have 1-16 non-empty labels"}),
                        true,
                    );
                }
                self.run(
                    session_id,
                    Work::Menu {
                        pid,
                        window_id,
                        path: path
                            .iter()
                            .map(|s| s.as_str().unwrap().to_owned())
                            .collect(),
                    },
                    wait_ms,
                    include_image,
                )
                .await
            }
            "desktop" => {
                let Some(cap) = obj
                    .get("max_image_dimension")
                    .map(Value::as_u64)
                    .unwrap_or(Some(1600))
                    .and_then(|n| u32::try_from(n).ok())
                else {
                    return mcp_text(
                        json!({"error":"max_image_dimension must be a non-negative u32"}),
                        true,
                    );
                };
                self.run(
                    session_id,
                    Work::Desktop {
                        max_image_dimension: cap,
                    },
                    wait_ms,
                    include_image,
                )
                .await
            }
            "permissions" => {
                match self
                    .driver
                    .call_tool("check_permissions".into(), "{\"prompt\":false}".into())
                    .await
                {
                    Ok(result) => {
                        let saved = save(result);
                        let structured =
                            serde_json::from_str::<Value>(&saved.structured).unwrap_or(Value::Null);
                        let (description, truncated) = short(&saved.text, MODEL_TEXT_BYTES);
                        mcp_text(
                            json!({"status":if saved.is_error {"refused"} else {"ok"},
                            "error_code":saved.error_code,"text":description,"text_truncated":truncated,
                            "details":structured}),
                            saved.is_error,
                        )
                    }
                    Err(e) => mcp_text(json!({"status":"error","error":e.to_string()}), true),
                }
            }
            "status" | "result" | "cancel" => self.control(session_id, operation, &args).await,
            _ => mcp_text(json!({"error":"invalid operation"}), true),
        }
    }

    async fn run(
        self: &Arc<Self>,
        session_id: &str,
        work: Work,
        wait_ms: u64,
        include_image: bool,
    ) -> Value {
        let (native, state, started) = {
            let mut sessions = self.sessions.lock().unwrap();
            if !sessions.contains_key(session_id) {
                let native = match self.driver.create_trusted_session(TrustedSessionOptions {
                    // CUA reserves the literal implicit/default label for transport leases.
                    // Every DTW session, including the omitted public default, is explicit.
                    public_session: format!("dtw-{session_id}"),
                    mode: SessionPermissionMode::Standard,
                    ttl_seconds: 3600,
                    idle_ttl_seconds: 1800,
                    capability_manifest_path: None,
                    bounded_manifest_path: None,
                }) {
                    Ok(native) => native,
                    Err(e) => return mcp_text(json!({"error":e.to_string()}), true),
                };
                sessions.insert(
                    session_id.to_owned(),
                    Session {
                        native,
                        state: "{}".into(),
                        busy: false,
                        started: false,
                    },
                );
            }
            let current = sessions.get_mut(session_id).unwrap();
            if current.busy {
                return mcp_text(json!({"error":"session_busy"}), true);
            }
            current.busy = true;
            (
                current.native.clone(),
                current.state.clone(),
                current.started,
            )
        };
        let execution_id = uuid::Uuid::new_v4().to_string();
        self.jobs.lock().unwrap().insert(
            execution_id.clone(),
            Job {
                session: session_id.to_owned(),
                phase: "running".into(),
                cancel_requested: false,
                result: None,
            },
        );
        let own = self.clone();
        let name = session_id.to_owned();
        let job_id = execution_id.clone();
        tokio::spawn(async move {
            let outcome = own.execute(native, work, &state, started, &job_id).await;
            let mut sessions = own.sessions.lock().unwrap();
            let current = sessions.get_mut(&name).unwrap();
            current.busy = false;
            if let Ok((_, Some(new_state), _)) = &outcome {
                current.state = new_state.clone();
            }
            if let Ok((_, _, true)) = &outcome {
                current.started = true;
            }
            drop(sessions);
            let mut jobs = own.jobs.lock().unwrap();
            let job = jobs.get_mut(&job_id).unwrap();
            let (saved, unknown) = match outcome {
                Ok((saved, _, _)) => (saved, false),
                Err(e) => {
                    let unknown = matches!(
                        e,
                        DriverError::ActionInterrupted {
                            completion: cua_driver_sdk::worker::ActionCompletion::Unknown,
                            ..
                        }
                    );
                    let code = match &e {
                        DriverError::ActionInterrupted { completion, .. } => {
                            format!("action_interrupted_{completion:?}").to_lowercase()
                        }
                        DriverError::Tool { error_code, .. } if !error_code.is_empty() => {
                            error_code.clone()
                        }
                        _ => "driver_error".into(),
                    };
                    let message = e.to_string();
                    (
                        SavedResult {
                            text: message.clone(),
                            structured: json!({"error":message,"code":code}).to_string(),
                            images: vec![],
                            is_error: true,
                            error_code: Some(code),
                        },
                        unknown,
                    )
                }
            };
            job.phase = if unknown {
                "unknown"
            } else if saved.error_code.as_deref() == Some("cancelled_before_action") {
                "cancelled"
            } else if saved.is_error {
                "failed"
            } else {
                "completed"
            }
            .into();
            job.result = Some(saved);
        });
        for _ in 0..wait_ms.div_ceil(25) {
            let terminal = self
                .jobs
                .lock()
                .unwrap()
                .get(&execution_id)
                .is_some_and(|job| job.phase != "running");
            if terminal {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let (terminal, has_image) = self
            .jobs
            .lock()
            .unwrap()
            .get(&execution_id)
            .map(|job| {
                (
                    job.phase != "running",
                    job.result
                        .as_ref()
                        .is_some_and(|result| !result.images.is_empty()),
                )
            })
            .unwrap_or((false, false));
        if terminal {
            return self
                .control(
                    session_id,
                    "result",
                    &json!({"execution_id":execution_id,"part":if include_image && has_image {"image"} else {"summary"}}),
                )
                .await;
        }
        mcp_text(
            json!({"execution_id":execution_id,"session":session_id,"status":"running"}),
            false,
        )
    }

    async fn execute(
        &self,
        native: Arc<CuaDriverSession>,
        work: Work,
        state: &str,
        started: bool,
        job_id: &str,
    ) -> Result<(SavedResult, Option<String>, bool), DriverError> {
        if !started {
            let start = native
                .call_tool("start_session".into(), "{}".into())
                .await?;
            if start.is_error {
                return Ok((save(start), None, false));
            }
        }
        if self
            .jobs
            .lock()
            .unwrap()
            .get(job_id)
            .is_some_and(|job| job.cancel_requested)
        {
            return Ok((
                SavedResult {
                    text: "cancelled before native action started".into(),
                    structured: json!({"code":"cancelled_before_action"}).to_string(),
                    images: vec![],
                    is_error: true,
                    error_code: Some("cancelled_before_action".into()),
                },
                None,
                true,
            ));
        }
        let code = match work {
            Work::Cursor(enabled) => {
                let (name, args) = match enabled {
                    Some(enabled) => ("set_agent_cursor_enabled", json!({"enabled":enabled})),
                    None => ("get_agent_cursor_state", json!({})),
                };
                let result = native.call_tool(name.into(), args.to_string()).await?;
                return Ok((save(result), None, true));
            }
            Work::Focus { pid, window_id } => {
                let mut args = json!({"pid":pid});
                if let Some(window_id) = window_id {
                    args["window_id"] = json!(window_id);
                }
                let result = native
                    .call_tool("bring_to_front".into(), args.to_string())
                    .await?;
                return Ok((save(result), None, true));
            }
            Work::Menu {
                pid,
                window_id,
                path,
            } => {
                let result = native
                    .call_tool(
                        "invoke_menu".into(),
                        json!({"pid":pid,"window_id":window_id,"path":path}).to_string(),
                    )
                    .await?;
                return Ok((save(result), None, true));
            }
            Work::Desktop {
                max_image_dimension,
            } => {
                let result = native
                    .call_tool(
                        "get_desktop_state".into(),
                        json!({"max_image_dimension":max_image_dimension}).to_string(),
                    )
                    .await?;
                return Ok((save(result), None, true));
            }
            Work::Script(code) => code,
        };
        let wrapped = format!(
            "const state = {state};\nconst __dtw_value = await (async () => {{\n{code}\n}})();\nreturn {{__dtw_value, __dtw_state: state}};"
        );
        let arguments = json!({"script":wrapped,"timeout_ms":120000,"max_calls":500}).to_string();
        let result = native.call_tool("run_script".into(), arguments).await?;
        let mut saved = save(result);
        let mut new_state = None;
        if !saved.is_error {
            if let Ok(mut value) = serde_json::from_str::<Value>(&saved.structured) {
                if let Some(wrapped_value) = value.get("value") {
                    if let Some(s) = wrapped_value.get("__dtw_state") {
                        let encoded = s.to_string();
                        if encoded.len() <= MAX_STATE_BYTES {
                            new_state = Some(encoded);
                        } else {
                            saved.is_error = true;
                            saved.error_code = Some("state_too_large".into());
                            saved.text = "script completed, but state exceeded 32 KiB; desktop effects may have occurred".into();
                        }
                    }
                    let unwrapped = wrapped_value
                        .get("__dtw_value")
                        .cloned()
                        .unwrap_or(Value::Null);
                    value["value"] = unwrapped;
                    saved.structured = value.to_string();
                }
            }
        }
        Ok((saved, new_state, true))
    }

    async fn control(&self, session_id: &str, operation: &str, args: &Value) -> Value {
        let Some(id) = args.get("execution_id").and_then(Value::as_str) else {
            return mcp_text(json!({"error":"execution_id is required"}), true);
        };
        let mut jobs = self.jobs.lock().unwrap();
        let Some(job) = jobs.get_mut(id) else {
            return mcp_text(
                json!({"error":"execution_not_found","execution_id":id}),
                true,
            );
        };
        if job.session != session_id {
            return mcp_text(
                json!({"error":"execution_not_found","execution_id":id}),
                true,
            );
        }
        if operation == "cancel" && job.phase == "running" {
            job.cancel_requested = true;
        }
        let mut summary = json!({
            "execution_id":id,"session":session_id,"status":job.phase,
            "cancel_requested":job.cancel_requested,
        });
        if let Some(result) = &job.result {
            let preview_limit = if operation == "result"
                && args
                    .get("part")
                    .and_then(Value::as_str)
                    .is_some_and(|part| part != "summary")
            {
                512
            } else {
                MODEL_TEXT_BYTES
            };
            let compacted = (!result.is_error)
                .then(|| compact_success_text(&result.text))
                .flatten();
            let display_text = compacted.as_deref().unwrap_or(&result.text);
            let (preview, truncated) = short(display_text, preview_limit);
            summary["error_code"] = json!(result.error_code);
            summary["is_error"] = json!(result.is_error);
            summary["text"] = json!(preview);
            summary["text_truncated"] = json!(truncated || compacted.is_some());
            summary["text_compacted"] = json!(compacted.is_some());
            summary["full_text_bytes"] = json!(result.text.len());
            summary["structured_bytes"] = json!(result.structured.len());
            summary["image_count"] = json!(result.images.len());
            if let Ok(structured) = serde_json::from_str::<Value>(&result.structured) {
                if let Some(activated) = structured.get("activated").and_then(Value::as_bool) {
                    summary["focus"] = json!({
                        "activated": activated,
                        "code": structured.get("code"),
                        "focused_window_id": structured.pointer("/observed/focused_window_id"),
                        "verified": structured.pointer("/exact_window_effect/verified"),
                    });
                }
                if let Some(capture_id) = structured.get("capture_id").and_then(Value::as_str) {
                    summary["capture"] = json!({
                        "capture_id": capture_id,
                        "display": structured.get("display"),
                        "screenshot_width": structured.get("screenshot_width"),
                        "screenshot_height": structured.get("screenshot_height"),
                        "screen_width": structured.get("screen_width"),
                        "screen_height": structured.get("screen_height"),
                    });
                }
                if let Some(value) = structured.get("value") {
                    let encoded = value.to_string();
                    if encoded.len() <= 4096 {
                        summary["value"] = value.clone();
                    } else {
                        summary["value_bytes"] = json!(encoded.len());
                    }
                }
                if let Some(error) = structured.get("error") {
                    let encoded = error.to_string();
                    let (preview, truncated) = short(&encoded, 2048);
                    summary["native_error"] = json!(preview);
                    summary["native_error_truncated"] = json!(truncated);
                }
            }
            if operation == "result" {
                let part = args
                    .get("part")
                    .and_then(Value::as_str)
                    .unwrap_or("summary");
                let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
                match part {
                    "text" => summary["page"] = page(&result.text, offset),
                    "structured" => summary["page"] = page(&result.structured, offset),
                    "image" => {
                        let index =
                            args.get("image_index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        if let Some(image) = result.images.get(index) {
                            return json!({"content":[
                                {"type":"text","text":summary.to_string()},
                                {"type":"image","mimeType":image.mime_type,"data":image.data_base64}
                            ],"isError":result.is_error});
                        }
                        return mcp_text(json!({"error":"image_index_out_of_range"}), true);
                    }
                    "summary" => {}
                    _ => return mcp_text(json!({"error":"invalid part"}), true),
                }
            }
            return mcp_text(summary, result.is_error);
        }
        if operation == "cancel" {
            summary["note"] = json!("Cancellation requested. Upstream private worker may finish the in-flight action; query this original receipt. No replay.");
        }
        mcp_text(summary, false)
    }
}

fn save(result: ToolResult) -> SavedResult {
    let structured = result.structured_json.unwrap_or_else(|| "null".into());
    let native_code = serde_json::from_str::<Value>(&structured)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/code")
                .or_else(|| value.pointer("/refusal/code"))
                .or_else(|| value.get("code"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    let error_code = if result.is_error {
        result.error_code.or(native_code)
    } else {
        None
    };
    SavedResult {
        text: result.text,
        structured,
        images: result.images,
        is_error: result.is_error,
        error_code,
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let worker = match worker_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let driver = match new_driver(worker) {
        Ok(driver) => driver,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let server = Arc::new(Server {
        driver: driver.clone(),
        sessions: Mutex::new(HashMap::new()),
        jobs: Mutex::new(HashMap::new()),
        stdout: AsyncMutex::new(io::stdout()),
    });
    let mut input = BufReader::new(io::stdin()).lines();
    while let Ok(Some(line)) = input.next_line().await {
        if let Ok(request) = serde_json::from_str::<Value>(&line) {
            let server = server.clone();
            tokio::spawn(async move {
                server.handle(request).await;
            });
        }
    }
    // The supervisor and trusted session handles own the private child's lifetime.
    server.sessions.lock().unwrap().clear();
    let _ = driver.shutdown().await;
}

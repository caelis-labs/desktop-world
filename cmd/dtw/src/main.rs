use cua_driver_sdk::{
    ConfiguredDriverOptions, CuaDriver, CuaDriverSession, DriverError, EmbeddedEnvironmentVariable,
    PrivateWorkerOptions, RuntimeAuthorizationOptions, SessionPermissionMode, ToolResult,
    TrustedSessionOptions,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Mutex as AsyncMutex,
};

mod app_view;
mod installer;
mod lifecycle;

const MAX_STATE_BYTES: usize = 32 * 1024;
const MODEL_TEXT_BYTES: usize = 12 * 1024;
const PAGE_BYTES: usize = 32 * 1024;
const EMBEDDED_BUNDLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded-bundle.tar.gz"));

fn executable_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

struct Session {
    native: Arc<CuaDriverSession>,
    state: String,
    busy: bool,
    started: bool,
    created: std::time::Instant,
    last_used: std::time::Instant,
}

// Extensions can do useful work without touching CUA, so cap absolute age below
// the upstream idle TTL too. Never transparently replay an expired-context action.
fn context_expired(age: Duration, idle: Duration) -> bool {
    age >= Duration::from_secs(1500) || idle >= Duration::from_secs(1500)
}

struct Job {
    phase: String,
    cancel_requested: bool,
    result: Option<SavedResult>,
    finished: Option<std::time::Instant>,
    expired: bool,
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
    App {
        pid: u32,
        max_image_dimension: u32,
        max_windows: usize,
    },
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
    AppClick {
        observation_id: String,
        image_index: usize,
        x: f64,
        y: f64,
    },
}

struct Server {
    driver: Arc<CuaDriver>,
    lifetime: lifecycle::Lifetime,
    session: Mutex<Option<Session>>,
    app_view: app_view::Runtime,
    jobs: Mutex<HashMap<String, Job>>,
    pending_requests: Mutex<HashSet<String>>,
    stdout: AsyncMutex<io::Stdout>,
}

fn worker_path() -> Result<String, String> {
    if let Some(arg) = env::args().skip(1).next() {
        if arg == "--help" || arg == "-h" || arg == "help" {
            println!(
                "Desktop World {}\n\n{}",
                env!("CARGO_PKG_VERSION"),
                installer::usage()
            );
            std::process::exit(0);
        }
        if arg == "--version" || arg == "version" {
            println!(
                "dtw {} (CUA 0.37.0 source 2d2f99228d4d)",
                env!("CARGO_PKG_VERSION")
            );
            std::process::exit(0);
        }
        if arg == "--worker" {
            return env::args()
                .nth(2)
                .ok_or_else(|| "--worker needs a binary path".to_owned());
        }
        if arg == "install" {
            let args: Vec<String> = env::args().collect();
            if args.get(2).map(String::as_str) == Some("--help") {
                println!("{}", installer::usage());
                std::process::exit(0);
            }
            if args.len() == 5 && args[2] == "plugin" && args[3] == "--to" {
                install_plugin(Path::new(&args[4]))?;
            } else {
                installer::run(&args[1..])?;
            }
            std::process::exit(0);
        }
        if arg == "uninstall" {
            let args: Vec<String> = env::args().collect();
            installer::run(&args[1..])?;
            std::process::exit(0);
        }
        return Err(format!("unknown argument: {arg}"));
    }
    if let Ok(path) = env::var("DTW_CUA_WORKER") {
        return Ok(path);
    }
    let path = package_root()?
        .join("libexec")
        .join(executable_name("cua-driver"));
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

// Export only release-owned content; host registrations and owner markers never travel.
fn canonical_copy(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination).map_err(|e| e.to_string())?;
    for name in ["bin", "libexec", "skills", "LICENSING"] {
        copy_tree(&source.join(name), &destination.join(name))?;
    }
    for name in [
        "LICENSE",
        "NOTICE",
        "THIRD_PARTY_NOTICES.md",
        "CUA-MIT-LICENSE.txt",
        "CUA-THIRD_PARTY_NOTICES.md",
    ] {
        fs::copy(source.join(name), destination.join(name)).map_err(|e| e.to_string())?;
    }
    fs::create_dir(destination.join(".claude-plugin")).map_err(|e| e.to_string())?;
    for (name, content) in [
        (
            "plugin.json",
            include_str!("../../../packaging/plugin/plugin.json"),
        ),
        (
            "mcp.json",
            include_str!("../../../packaging/plugin/mcp.json"),
        ),
        (
            ".mcp.json",
            include_str!("../../../packaging/plugin/.mcp.json"),
        ),
        (
            "gemini-extension.json",
            include_str!("../../../packaging/plugin/gemini-extension.json"),
        ),
        (
            ".claude-plugin/plugin.json",
            include_str!("../../../packaging/plugin/.claude-plugin/plugin.json"),
        ),
    ] {
        let content = if cfg!(windows) {
            content
                .replace("bin/dtw\"", "bin/dtw.exe\"")
                .replace("bin${/}dtw\"", "bin${/}dtw.exe\"")
        } else {
            content.to_owned()
        };
        fs::write(destination.join(name), content).map_err(|e| e.to_string())?;
    }
    // Skills may have installer ownership markers in direct host projections.
    let _ = fs::remove_file(destination.join("skills/desktop-world/.dtw-owner.json"));
    write_sums(destination)
}

fn package_root() -> Result<PathBuf, String> {
    let binary = env::current_exe().map_err(|e| e.to_string())?;
    let adjacent = binary
        .parent()
        .and_then(|p| p.parent())
        .ok_or("cannot locate package root")?;
    if adjacent.join("bin").join(executable_name("dtw")) == binary && valid_package(adjacent) {
        return Ok(adjacent.to_path_buf());
    }
    expand_embedded(&binary)
}

fn valid_package(package: &Path) -> bool {
    for required in [
        PathBuf::from("libexec").join(executable_name("cua-driver")),
        PathBuf::from("skills/desktop-world/SKILL.md"),
        PathBuf::from("mcp.json"),
        PathBuf::from("LICENSING/DEPENDENCIES.json"),
        PathBuf::from("LICENSING/MPL-SOURCES.tar.gz"),
    ] {
        if !package.join(&required).is_file() {
            return false;
        }
    }
    true
}

fn data_root() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let home = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME"));
    #[cfg(not(windows))]
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"));
    let home = PathBuf::from(home.ok_or("HOME or USERPROFILE is required")?);
    #[cfg(target_os = "macos")]
    let base = home.join("Library/Application Support");
    #[cfg(windows)]
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData/Local"));
    #[cfg(not(any(target_os = "macos", windows)))]
    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    Ok(base.join("desktop-world"))
}

fn write_sums(root: &Path) -> Result<(), String> {
    fn walk(root: &Path, dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, paths)?;
            } else if path.is_file()
                && path.file_name().and_then(|s| s.to_str()) != Some("SHA256SUMS")
            {
                paths.push(
                    path.strip_prefix(root)
                        .map_err(|e| e.to_string())?
                        .to_path_buf(),
                );
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    walk(root, root, &mut paths)?;
    paths.sort();
    let mut lines = String::new();
    for path in paths {
        let bytes = fs::read(root.join(&path)).map_err(|e| e.to_string())?;
        lines.push_str(&format!(
            "{:x}  {}\n",
            Sha256::digest(&bytes),
            path.to_string_lossy().replace('\\', "/")
        ));
    }
    fs::write(root.join("SHA256SUMS"), lines).map_err(|e| e.to_string())
}

fn expand_embedded(binary: &Path) -> Result<PathBuf, String> {
    if EMBEDDED_BUNDLE.is_empty() {
        return Err("this development binary has no embedded runtime; use a release package or scripts/build-plugin".into());
    }
    let digest = format!("{:x}", Sha256::digest(EMBEDDED_BUNDLE));
    // The runtime bundle can stay byte-identical across CLI-only updates. Include
    // the executing binary so a newer installer never reuses a cached old CLI.
    let binary_digest = format!(
        "{:x}",
        Sha256::digest(fs::read(binary).map_err(|e| e.to_string())?)
    );
    let root = data_root()?.join("runtime").join(format!(
        "{}-{}-{}",
        env!("CARGO_PKG_VERSION"),
        &digest[..16],
        &binary_digest[..16]
    ));
    let marker = root.join(".dtw-bundle-sha256");
    if root.exists() {
        if fs::read_to_string(&marker)
            .map_err(|e| e.to_string())?
            .trim()
            != digest
            || !valid_package(&root)
        {
            return Err(format!(
                "embedded runtime cache is invalid: {}",
                root.display()
            ));
        }
        return Ok(root);
    }
    let parent = root.parent().ok_or("invalid runtime cache path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stage = parent.join(format!(".dtw-runtime-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        fs::create_dir(&stage).map_err(|e| e.to_string())?;
        let gz = flate2::read::GzDecoder::new(EMBEDDED_BUNDLE);
        let mut archive = tar::Archive::new(gz);
        for entry in archive.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            if !entry.unpack_in(&stage).map_err(|e| e.to_string())? {
                return Err("embedded bundle has an unsafe path".into());
            }
        }
        fs::create_dir_all(stage.join("bin")).map_err(|e| e.to_string())?;
        fs::copy(binary, stage.join("bin").join(executable_name("dtw")))
            .map_err(|e| e.to_string())?;
        if !valid_package(&stage) {
            return Err("embedded runtime is incomplete".into());
        }
        fs::write(stage.join(".dtw-bundle-sha256"), format!("{digest}\n"))
            .map_err(|e| e.to_string())?;
        write_sums(&stage)?;
        match fs::rename(&stage, &root) {
            Ok(()) => Ok(()),
            Err(e) if root.exists() => {
                let _ = fs::remove_dir_all(&stage);
                if fs::read_to_string(&marker)
                    .map_err(|e| e.to_string())?
                    .trim()
                    == digest
                    && valid_package(&root)
                {
                    Ok(())
                } else {
                    Err(format!("runtime cache race: {e}"))
                }
            }
            Err(e) => Err(e.to_string()),
        }
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result?;
    Ok(root)
}

fn install_plugin(destination: &Path) -> Result<(), String> {
    let package = package_root()?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    let leaf = destination.file_name().ok_or("invalid destination")?;
    let destination = parent.join(leaf);
    if destination.starts_with(package.canonicalize().map_err(|e| e.to_string())?) {
        return Err("export destination must be outside the source package".into());
    }
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(format!(
            "destination already exists: {}",
            destination.display()
        ));
    }
    let staging =
        destination.with_file_name(format!(".desktop-world-install-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        canonical_copy(&package, &staging)?;
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
        json!({"plugin":destination,"mcp_command":destination.join("bin").join(executable_name("dtw")),"format":"agent-plugin-1.0"})
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

fn new_driver(path: String, lifetime: &lifecycle::Lifetime) -> Result<Arc<CuaDriver>, String> {
    if !PathBuf::from(&path).is_file() {
        return Err(format!("private CUA worker missing: {path}"));
    }
    CuaDriver::create_private_worker(PrivateWorkerOptions {
        binary_path: env::current_exe()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned(),
        host_bundle_id: "dev.caelis.dtw".into(),
        startup_timeout_ms: Some(15_000),
        shutdown_timeout_ms: Some(3_000),
        configured_driver: configured_driver(),
        environment: ["TMPDIR", "TMP", "TEMP"]
            .iter()
            .map(|name| EmbeddedEnvironmentVariable {
                name: (*name).into(),
                value: lifetime.root.to_string_lossy().into_owned(),
            })
            .collect(),
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

fn mcp_plain(value: impl Into<String>, is_error: bool) -> Value {
    json!({"content":[{"type":"text","text":value.into()}], "isError":is_error})
}

fn short(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}...truncated {} chars...",
        &s[..end],
        s[end..].chars().count()
    )
}

fn display_result(result: &SavedResult) -> String {
    let mut text = result.text.clone();
    if result.is_error || !text.starts_with("run_script: ok") {
        return text;
    }
    // CUA's return line contains our private state wrapper. Replace that
    // line in place, retaining the native order of status, value and calls.
    let mut returned = String::new();
    if let Ok(structured) = serde_json::from_str::<Value>(&result.structured) {
        if let Some(value) = structured.get("value").filter(|v| !v.is_null()) {
            // CUA already names each action and its result in the call trace.
            // Repeating its action object adds fields without an action fact.
            let action_in_calls = text.contains("\ncalls:\n")
                && value.get("effect").is_some()
                && value.get("route").is_some();
            if !action_in_calls {
                returned = format!("\nreturn value: {value}");
            }
        }
    }
    if let Some(start) = text.find("\nreturn value:") {
        let end = text[start + 1..]
            .find('\n')
            .map(|n| start + 1 + n)
            .unwrap_or(text.len());
        text.replace_range(start..end, &returned);
    }
    text
}

#[cfg(test)]
mod receipt_tests {
    #[test]
    fn context_rotation_precedes_upstream_idle_expiry_even_with_extension_activity() {
        use std::time::Duration;
        assert!(!super::context_expired(
            Duration::from_secs(1499),
            Duration::ZERO
        ));
        assert!(super::context_expired(
            Duration::from_secs(1500),
            Duration::ZERO
        ));
        assert!(super::context_expired(
            Duration::ZERO,
            Duration::from_secs(1500)
        ));
    }

    use super::*;

    #[test]
    fn native_action_text_is_not_repeated_as_json() {
        let result = SavedResult {
            text: "run_script: ok in 1021 ms, 1 driver call(s)\nreturn value: {\"__dtw_value\":{},\"__dtw_state\":{}}\ncalls:\n  1. line 3 press_key ok (16 ms): Pressed 'return' on the desktop.".into(),
            structured: json!({"value":{"effect":"unverifiable","route":"global_input","summary":"Pressed 'return' on the desktop.","text":"Pressed 'return' on the desktop."}}).to_string(),
            images: vec![],
            is_error: false,
            error_code: None,
        };
        assert_eq!(display_result(&result), "run_script: ok in 1021 ms, 1 driver call(s)\ncalls:\n  1. line 3 press_key ok (16 ms): Pressed 'return' on the desktop.");
    }

    #[test]
    fn truncation_reports_characters_in_the_text() {
        assert_eq!(short("é中ab", 3), "é...truncated 3 chars...");
    }
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
    json!({"data": &s[start..end], "offset":start, "next_offset": if end < s.len() {Some(end)} else {None}})
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
                    "serverInfo":{"name":"dtw", "version":env!("CARGO_PKG_VERSION")}
                }),
            ),
            "ping" => reply(id, json!({})),
            "tools/list" => reply(
                id,
                json!({"tools":[{
                    "name":"exec",
                    "description":"Run CUA's native QuickJS script or its native primitives. operation=app observes attributed app windows (default top2 images); operation=click consumes its observation_id and image pixel point. Runtime owns the session. Attribution is best effort. Use the short Skill and api.d.ts for signatures. Fetch retained images with operation=result,part=image.",
                    "inputSchema":{
                        "type":"object",
                        "properties":{
                            "observation_id":{"type":"string","description":"With click, the one-use application observation binding."},
                            "x":{"type":"number","minimum":0},
                            "y":{"type":"number","minimum":0},
                            "max_windows":{"type":"integer","minimum":1,"maximum":8,"description":"With app, number of upper windows to capture, default 2; all attributed candidates are listed."},
                            "script":{"type":"string","description":"Async CUA run_script body. Use cua.getApp, getState, query, click, typeText, pressKey, zoom and return; see packaged api.d.ts."},
                            "operation":{"type":"string","enum":["run","status","result","cancel","permissions","cursor","focus","menu","desktop","app","click"]},
                            "wait_ms":{"type":"integer","minimum":0,"maximum":5000,"description":"Wait briefly for a terminal receipt in the same exec call (default 4000 ms). If still running, query the original id with operation=result."},
                            "include_image":{"type":"boolean","description":"If this execution completes within wait_ms and has an image, return the first image with its summary. Default false; later result,part=image still works."},
                            "enabled":{"type":"boolean","description":"With operation=cursor, show or hide the runtime virtual cursor. Omit to read cursor state."},
                            "pid":{"type":"integer","minimum":1,"description":"With focus/menu/app, exact app PID."},
                            "window_id":{"type":"integer","minimum":1,"description":"With menu, exact target window; with focus, optional ordinary window ID."},
                            "path":{"type":"array","minItems":1,"maxItems":16,"items":{"type":"string"},"description":"With menu, exact macOS menu labels from menu bar to command."},
                            "max_image_dimension":{"type":"integer","minimum":0,"description":"With desktop/app, optional screenshot long-edge cap (default 1600, 0 for original size); image is retained for result,part=image."},
                            "id":{"type":"string","description":"The original short receipt ID for status, result, or cancel."},
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
        self.pending_requests
            .lock()
            .unwrap()
            .remove(&id.to_string());
    }

    async fn exec(self: &Arc<Self>, args: Value) -> Value {
        let Some(obj) = args.as_object() else {
            return mcp_plain("arguments must be an object", true);
        };
        if obj.contains_key("session") {
            return mcp_plain("session_managed_by_runtime: omit session; use a separate MCP process for an independent context", true);
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
                    self.run(Work::Script(script.to_owned()), wait_ms, include_image)
                        .await
                }
                None => mcp_plain("script is required", true),
            },
            "cursor" => {
                self.run(
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
                    return mcp_plain("focus requires positive pid", true);
                };
                let window_id = obj.get("window_id").and_then(Value::as_u64);
                if obj.get("window_id").is_some() && window_id.is_none_or(|n| n == 0) {
                    return mcp_plain("window_id must be positive", true);
                }
                self.run(Work::Focus { pid, window_id }, wait_ms, include_image)
                    .await
            }
            "menu" => {
                let Some(pid) = obj
                    .get("pid")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0)
                else {
                    return mcp_plain("menu requires positive pid", true);
                };
                let Some(window_id) = obj
                    .get("window_id")
                    .and_then(Value::as_u64)
                    .filter(|n| *n > 0)
                else {
                    return mcp_plain("menu requires positive window_id", true);
                };
                let Some(path) = obj.get("path").and_then(Value::as_array) else {
                    return mcp_plain("menu requires path", true);
                };
                if path.is_empty()
                    || path.len() > 16
                    || path
                        .iter()
                        .any(|s| s.as_str().is_none_or(|s| s.trim().is_empty()))
                {
                    return mcp_plain("menu path must have 1-16 non-empty labels", true);
                }
                self.run(
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
            "click" => {
                let Some(observation_id) = obj
                    .get("observation_id")
                    .and_then(Value::as_str)
                    .filter(|s| s.len() == 32)
                else {
                    return mcp_plain("click requires observation_id", true);
                };
                let Some(image_index) = obj
                    .get("image_index")
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                else {
                    return mcp_plain("click requires image_index", true);
                };
                let (Some(x), Some(y)) = (
                    obj.get("x").and_then(Value::as_f64),
                    obj.get("y").and_then(Value::as_f64),
                ) else {
                    return mcp_plain("click requires image pixel x/y", true);
                };
                self.run(
                    Work::AppClick {
                        observation_id: observation_id.into(),
                        image_index,
                        x,
                        y,
                    },
                    wait_ms,
                    include_image,
                )
                .await
            }
            "app" => {
                let Some(pid) = obj
                    .get("pid")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0 && *n <= i32::MAX as u32)
                else {
                    return mcp_plain("app requires positive process pid", true);
                };
                let Some(cap) = obj
                    .get("max_image_dimension")
                    .map(Value::as_u64)
                    .unwrap_or(Some(1600))
                    .and_then(|n| u32::try_from(n).ok())
                else {
                    return mcp_plain("max_image_dimension must be non-negative u32", true);
                };
                let Some(max_windows) = obj
                    .get("max_windows")
                    .map(Value::as_u64)
                    .unwrap_or(Some(2))
                    .filter(|n| (1..=8).contains(n))
                else {
                    return mcp_plain("max_windows must be 1..8", true);
                };
                self.run(
                    Work::App {
                        pid,
                        max_image_dimension: cap,
                        max_windows: max_windows as usize,
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
                    return mcp_plain("max_image_dimension must be a non-negative u32", true);
                };
                self.run(
                    Work::Desktop {
                        max_image_dimension: cap,
                    },
                    wait_ms,
                    include_image,
                )
                .await
            }
            "permissions" => {
                // Windows accepts no arguments; prompt is a macOS-only option.
                let arguments = if cfg!(target_os = "macos") {
                    "{\"prompt\":false}"
                } else {
                    "{}"
                };
                match self
                    .driver
                    .call_tool("check_permissions".into(), arguments.into())
                    .await
                {
                    Ok(result) => {
                        let saved = save(result);
                        mcp_plain(short(&saved.text, MODEL_TEXT_BYTES), saved.is_error)
                    }
                    Err(e) => mcp_plain(e.to_string(), true),
                }
            }
            "status" | "result" | "cancel" => self.control(operation, &args).await,
            _ => mcp_plain("invalid operation", true),
        }
    }

    async fn run(self: &Arc<Self>, work: Work, wait_ms: u64, include_image: bool) -> Value {
        if self.jobs.lock().unwrap().len() >= 4096 {
            return mcp_plain(
                "receipt_capacity_reached: restart Runtime after reconciling existing receipts",
                true,
            );
        }
        if self.lifetime.stopped() {
            return mcp_plain("runtime_stopped: restart Runtime and observe before any further input; never replay an unknown action", true);
        }
        let (native, state, started) = {
            let mut session = self.session.lock().unwrap();
            if session.as_ref().is_some_and(|s| {
                !s.busy && (context_expired(s.created.elapsed(), s.last_used.elapsed()))
            }) {
                *session = None;
                self.app_view.invalidate();
                return mcp_plain("runtime_context_reset: native context expired; state and observations were cleared; observe again before input", true);
            }
            if session.is_none() {
                let native = match self.driver.create_trusted_session(TrustedSessionOptions {
                    public_session: format!("dtw-{}", uuid::Uuid::new_v4().simple()),
                    mode: SessionPermissionMode::Standard,
                    ttl_seconds: 3600,
                    idle_ttl_seconds: 1800,
                    capability_manifest_path: None,
                    bounded_manifest_path: None,
                }) {
                    Ok(native) => native,
                    Err(e) => return mcp_plain(e.to_string(), true),
                };
                *session = Some(Session {
                    native,
                    state: "{}".into(),
                    busy: false,
                    started: false,
                    created: std::time::Instant::now(),
                    last_used: std::time::Instant::now(),
                });
            }
            let current = session.as_mut().unwrap();
            if current.busy {
                return mcp_plain("runtime_busy", true);
            }
            current.busy = true;
            (
                current.native.clone(),
                current.state.clone(),
                current.started,
            )
        };
        // IDs are scoped to this process. Retry a collision against its
        // receipt map so eight hex characters remain safe for live jobs.
        let id = {
            let mut jobs = self.jobs.lock().unwrap();
            loop {
                let uuid = uuid::Uuid::new_v4().simple().to_string();
                let id = uuid[..8].to_owned();
                if jobs.contains_key(&id) {
                    continue;
                }
                jobs.insert(
                    id.clone(),
                    Job {
                        phase: "running".into(),
                        cancel_requested: false,
                        result: None,
                        finished: None,
                        expired: false,
                    },
                );
                break id;
            }
        };
        let own = self.clone();
        let job_id = id.clone();
        tokio::spawn(async move {
            let outcome = own.execute(native, work, &state, started, &job_id).await;
            let mut session = own.session.lock().unwrap();
            let current = session.as_mut().unwrap();
            current.last_used = std::time::Instant::now();
            current.busy = false;
            if let Ok((_, Some(new_state), _)) = &outcome {
                current.state = new_state.clone();
            }
            if let Ok((_, _, true)) = &outcome {
                current.started = true;
            }
            drop(session);
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
            job.phase = if job.cancel_requested
                || unknown
                || saved.error_code.as_deref() == Some("input_outcome_unknown")
            {
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
            job.finished = Some(std::time::Instant::now());
            prune_results(&mut jobs);
        });
        for _ in 0..wait_ms.div_ceil(25) {
            let terminal = self
                .jobs
                .lock()
                .unwrap()
                .get(&id)
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
            .get(&id)
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
                    "result",
                    &json!({"id":id,"part":if include_image && has_image {"image"} else {"summary"}}),
                )
                .await;
        }
        mcp_plain(format!("id={id} status=running"), false)
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
        if !matches!(&work, Work::App { .. } | Work::AppClick { .. }) {
            self.app_view.invalidate();
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
            Work::AppClick {
                observation_id,
                image_index,
                x,
                y,
            } => {
                return Ok((
                    app_view::click(&self.app_view, &native, &observation_id, image_index, x, y)
                        .await?,
                    None,
                    true,
                ));
            }
            Work::App {
                pid,
                max_image_dimension,
                max_windows,
            } => {
                return Ok((
                    app_view::capture(
                        &self.app_view,
                        &native,
                        pid,
                        max_image_dimension,
                        max_windows,
                    )
                    .await?,
                    None,
                    true,
                ));
            }
            Work::Script(code) => code,
        };
        // Function-constructor frames have their own source identity in QuickJS,
        // so CUA's first script.js stack frame stays the Agent's call site.
        // Open the user's body on line 1, just as CUA's own engine does.
        let adapter = serde_json::to_string(&format!(
            "{}\nreturn __dtw_click_api(native);",
            include_str!("click_contract.js")
        ))
        .expect("encode adapter source");
        let wrapped = format!(
            "const state = {state};const __dtw_value = await (async (cua) => {{{code}\n}})(Function(\"native\", {adapter})(cua));return {{__dtw_value, __dtw_state: state}};"
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

    async fn control(&self, operation: &str, args: &Value) -> Value {
        let Some(id) = args.get("id").and_then(Value::as_str) else {
            return mcp_plain("id is required", true);
        };
        let mut jobs = self.jobs.lock().unwrap();
        prune_results(&mut jobs);
        let Some(job) = jobs.get_mut(id) else {
            return mcp_plain("execution_not_found", true);
        };
        if operation == "cancel" && job.phase == "running" {
            if let Err(error) = self.lifetime.stop() {
                return mcp_plain(format!("stop_failed: {error}"), true);
            }
            job.cancel_requested = true;
        }
        let mut line = format!("id={id} status={}", job.phase);
        if job.cancel_requested {
            line.push_str(" cancel_requested");
        }
        if job.expired && operation == "result" {
            return mcp_plain(format!("{line} result_expired: payload evicted; do not repeat the action; observe to reconcile"), true);
        }
        if let Some(result) = &job.result {
            if let Some(code) = result.error_code.as_deref() {
                line.push_str(&format!(" code={code}"));
            }
            if operation == "status" || operation == "cancel" {
                return mcp_plain(line, result.is_error);
            }
            if operation == "result" {
                let part = args
                    .get("part")
                    .and_then(Value::as_str)
                    .unwrap_or("summary");
                let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
                match part {
                    "text" => {
                        return mcp_text(
                            json!({"id":id,"status":job.phase,"page":page(&display_result(result), offset)}),
                            result.is_error,
                        )
                    }
                    "structured" => {
                        return mcp_text(
                            json!({"id":id,"status":job.phase,"page":page(&result.structured, offset)}),
                            result.is_error,
                        )
                    }
                    "image" => {
                        let index =
                            args.get("image_index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        if let Some(image) = result.images.get(index) {
                            let body = short(&display_result(result), MODEL_TEXT_BYTES);
                            if !body.is_empty() {
                                line.push('\n');
                                line.push_str(&body);
                            }
                            return json!({"content":[
                                {"type":"text","text":line},
                                {"type":"image","mimeType":image.mime_type,"data":image.data_base64}
                            ],"isError":result.is_error});
                        }
                        return mcp_plain("image_index_out_of_range", true);
                    }
                    "summary" => {}
                    _ => return mcp_plain("invalid part", true),
                }
            }
            let body = short(&display_result(result), MODEL_TEXT_BYTES);
            if !body.is_empty() {
                line.push('\n');
                line.push_str(&body);
            }
            return mcp_plain(line, result.is_error);
        }
        if operation == "cancel" {
            line.push_str(
                "\nQuery this id for the final result; an in-flight action may still finish.",
            );
        }
        mcp_plain(line, false)
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
    if lifecycle::supervisor_entry() || app_view::helper_entry() {
        return;
    }
    let worker = match worker_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let lifetime = lifecycle::Lifetime::new(&worker).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    let driver = match new_driver(worker, &lifetime) {
        Ok(driver) => driver,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let server = Arc::new(Server {
        driver: driver.clone(),
        lifetime,
        session: Mutex::new(None),
        app_view: app_view::Runtime::default(),
        jobs: Mutex::new(HashMap::new()),
        pending_requests: Mutex::new(HashSet::new()),
        stdout: AsyncMutex::new(io::stdout()),
    });
    let mut input = BufReader::new(io::stdin()).lines();
    loop {
        let line = tokio::select! {
            line = input.next_line() => match line { Ok(Some(line)) => line, _ => break },
            _ = shutdown_signal() => break,
        };
        if let Ok(request) = serde_json::from_str::<Value>(&line) {
            if request["method"] == "notifications/cancelled" {
                if let Some(id) = request.pointer("/params/requestId") {
                    if server
                        .pending_requests
                        .lock()
                        .unwrap()
                        .contains(&id.to_string())
                    {
                        if server.lifetime.stop().is_ok() {
                            for job in server
                                .jobs
                                .lock()
                                .unwrap()
                                .values_mut()
                                .filter(|j| j.phase == "running")
                            {
                                job.cancel_requested = true;
                            }
                        }
                    }
                }
                continue;
            }
            if request["method"] == "tools/call" {
                if let Some(id) = request.get("id") {
                    server
                        .pending_requests
                        .lock()
                        .unwrap()
                        .insert(id.to_string());
                }
            }
            let server = server.clone();
            tokio::spawn(async move {
                server.handle(request).await;
            });
        }
    }
    // The supervisor and trusted session handles own the private child's lifetime.
    let _ = server.lifetime.stop();
    let _ = tokio::time::timeout(Duration::from_secs(3), driver.shutdown()).await;
    // Blocking SDK tasks must not hold process exit after bounded shutdown.
    std::process::exit(0);
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! { _ = term.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

// Payloads: 64 MiB / 64 results / 15 minutes. Lightweight receipts remain bounded
// by refusing admission rather than forgetting an uncertain action's identity.
fn prune_results(jobs: &mut HashMap<String, Job>) {
    let mut completed: Vec<_> = jobs
        .iter()
        .filter_map(|(id, j)| j.finished.map(|t| (id.clone(), t)))
        .collect();
    completed.sort_by_key(|(_, t)| *t);
    let bytes = |r: &SavedResult| {
        r.text.len()
            + r.structured.len()
            + r.images
                .iter()
                .map(|i| i.data_base64.len() + i.mime_type.len())
                .sum::<usize>()
    };
    let mut total: usize = jobs
        .values()
        .filter_map(|j| j.result.as_ref())
        .map(bytes)
        .sum();
    let mut count = jobs.values().filter(|j| j.result.is_some()).count();
    for (id, time) in completed {
        let job = jobs.get_mut(&id).unwrap();
        if total > 64 * 1024 * 1024 || count > 64 || time.elapsed() >= Duration::from_secs(900) {
            if let Some(result) = job.result.take() {
                total -= bytes(&result);
                count -= 1;
                job.expired = true;
            }
        }
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    #[test]
    fn payload_eviction_preserves_unknown_receipts_and_running_jobs() {
        let mut jobs = HashMap::new();
        for n in 0..70 {
            jobs.insert(
                n.to_string(),
                Job {
                    phase: "unknown".into(),
                    cancel_requested: true,
                    result: Some(SavedResult {
                        text: "result".into(),
                        structured: "{}".into(),
                        images: vec![],
                        is_error: true,
                        error_code: None,
                    }),
                    finished: Some(std::time::Instant::now()),
                    expired: false,
                },
            );
        }
        jobs.insert(
            "active".into(),
            Job {
                phase: "running".into(),
                cancel_requested: false,
                result: None,
                finished: None,
                expired: false,
            },
        );
        prune_results(&mut jobs);
        assert_eq!(jobs.len(), 71);
        assert_eq!(jobs.values().filter(|j| j.result.is_some()).count(), 64);
        assert_eq!(jobs.values().filter(|j| j.expired).count(), 6);
        assert_eq!(jobs["active"].phase, "running");
        for job in jobs.values().filter(|j| j.expired) {
            assert_eq!(job.phase, "unknown");
        }
    }
    #[test]
    fn oversized_and_old_results_expire_without_replay() {
        let mut jobs = HashMap::new();
        jobs.insert(
            "large".into(),
            Job {
                phase: "completed".into(),
                cancel_requested: false,
                result: Some(SavedResult {
                    text: "x".repeat(65 * 1024 * 1024),
                    structured: "{}".into(),
                    images: vec![],
                    is_error: false,
                    error_code: None,
                }),
                finished: Some(std::time::Instant::now()),
                expired: false,
            },
        );
        prune_results(&mut jobs);
        assert!(jobs["large"].expired);
        assert!(jobs["large"].result.is_none());
    }
}

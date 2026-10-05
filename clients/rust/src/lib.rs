//! Language-neutral dtw session. Only trusted host code owns HostSession.
//! DesktopClient cannot submit host control operations. No automatic replay.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, Notify},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fault {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retry_class: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reply<T = Value> {
    pub id: String,
    pub protocol: String,
    pub world: String,
    #[serde(default)]
    pub result: Option<T>,
    #[serde(default)]
    pub error: Option<Fault>,
}
#[derive(Clone, Debug)]
pub struct Error {
    pub code: String,
    pub message: String,
    pub reply: Option<Reply>,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl Error {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            reply: None,
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Fact<T> {
    Compact {
        known: T,
    },
    Full {
        status: String,
        #[serde(default)]
        value: Option<T>,
        #[serde(flatten)]
        extra: HashMap<String, Value>,
    },
}
impl<T> Fact<T> {
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Compact { known } => Some(known),
            Self::Full { status, value, .. } if status == "known" => value.as_ref(),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UIObject {
    pub r#ref: String,
    pub kind: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub name: Option<Fact<String>>,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub window: Option<String>,
    #[serde(default)]
    pub value_preview: Option<Fact<String>>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub geometry_version: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Coverage {
    pub complete: bool,
    #[serde(default)]
    pub dirty: bool,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub continuation: Option<String>,
    #[serde(default)]
    pub unavailable_sources: Vec<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    #[serde(skip)]
    pub query: Value,
    pub objects: Vec<UIObject>,
    pub coverage: Coverage,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepResult {
    pub id: String,
    pub state: String,
    pub delivery: String,
    pub verification: String,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub run_id: String,
    pub outcome: String,
    pub steps: Vec<StepResult>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grant {
    pub id: String,
    #[serde(default)]
    pub application: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    pub state: String,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrantStatus {
    pub turn: String,
    pub grants: Vec<Grant>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Target {
    Ref { r#ref: String },
    Bound { bound: String },
}
impl Target {
    pub fn reference(value: impl Into<String>) -> Self {
        Self::Ref {
            r#ref: value.into(),
        }
    }
    pub fn bound(value: impl Into<String>) -> Self {
        Self::Bound {
            bound: value.into(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Plan {
    pub steps: Vec<Value>,
}
impl Plan {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, mut step: Value) -> &mut Self {
        if let Some(object) = step.as_object_mut() {
            object
                .entry("id")
                .or_insert(json!(format!("s{}", self.steps.len() + 1)));
        }
        self.steps.push(step);
        self
    }
    pub fn focus(&mut self, reference: &str) -> &mut Self {
        self.add(json!({"op":"focus","target":{"ref":reference}}))
    }
    pub fn bind_focus(&mut self, name: &str, within: &str) -> Target {
        self.add(json!({"op":"bind_focus","bind_focus":{"name":name,"within":within}}));
        Target::bound(name)
    }
    pub fn bind(&mut self, name: &str, locator: Value) -> Target {
        self.add(json!({"op":"bind","bind":{"name":name,"locator":locator,"require_unique":true}}));
        Target::bound(name)
    }
    pub fn press(&mut self, target: Target, key: &str, modifiers: &[&str]) -> &mut Self {
        self.add(json!({"op":"keyboard.press","target":target,"press":{"key":key,"modifiers":modifiers}}))
    }
    pub fn type_text(&mut self, target: Target, text: &str) -> &mut Self {
        self.add(json!({"op":"keyboard.type_text","target":target,"type_text":{"text":text}}))
    }
    pub fn set(&mut self, reference: &str, text: &str) -> &mut Self {
        self.add(json!({"op":"set_value","target":{"ref":reference},"set_value":{"text":text}}))
    }
    pub fn invoke(&mut self, reference: &str) -> &mut Self {
        self.add(json!({"op":"invoke","target":{"ref":reference}}))
    }
    pub fn click(&mut self, target: Target) -> &mut Self {
        self.add(json!({"op":"pointer.click","target":target,"click":{"button":"left","count":1}}))
    }
}
#[derive(Clone, Debug)]
pub struct SessionOptions {
    pub helper: PathBuf,
    pub input_mode: String,
    pub input_policy: Option<String>,
    pub write_apps: Vec<String>,
    pub write_app_windows: Vec<String>,
    pub assets_dir: Option<PathBuf>,
    pub audit: Option<PathBuf>,
    pub owner_file: Option<PathBuf>,
    pub audit_mode: Option<String>,
}
impl SessionOptions {
    pub fn new(helper: impl AsRef<Path>) -> Self {
        Self {
            helper: helper.as_ref().into(),
            input_mode: "cooperative".into(),
            input_policy: None,
            write_apps: vec![],
            write_app_windows: vec![],
            assets_dir: None,
            audit: None,
            owner_file: None,
            audit_mode: None,
        }
    }
}
struct Record {
    body: Vec<u8>,
    reply: Mutex<Option<Result<Reply>>>,
    notify: Notify,
}
struct State {
    records: HashMap<String, Arc<Record>>,
    broken: Option<Error>,
}
enum Write {
    Frame(Vec<u8>),
    Close,
}
struct Transport {
    state: Arc<Mutex<State>>,
    writer: mpsc::UnboundedSender<Write>,
    sequence: AtomicU64,
    child: tokio::sync::Mutex<Child>,
}
impl Transport {
    fn submit(
        &self,
        channel: &str,
        op: &str,
        args: Value,
        id: Option<&str>,
    ) -> Result<Arc<Record>> {
        let id = id
            .map(str::to_owned)
            .unwrap_or_else(|| format!("rs-{}", self.sequence.fetch_add(1, Ordering::Relaxed)));
        let mut body = serde_json::to_vec(&json!({"id":id,"channel":channel,"op":op,"args":args}))
            .map_err(|e| Error::new("invalid_argument", e.to_string()))?;
        body.push(b'\n');
        let mut state = self.state.lock().unwrap();
        if let Some(record) = state.records.get(&id) {
            if record.body != body {
                return Err(Error::new(
                    "request_conflict",
                    "Reuse ID only with identical arguments.",
                ));
            }
            return Ok(record.clone());
        }
        if let Some(error) = &state.broken {
            return Err(error.clone());
        }
        if state.records.len() >= 4096 {
            return Err(Error::new(
                "resource_exhausted",
                "Retain receipts before ending session.",
            ));
        }
        let record = Arc::new(Record {
            body: body.clone(),
            reply: Mutex::new(None),
            notify: Notify::new(),
        });
        state.records.insert(id, record.clone());
        if self.writer.send(Write::Frame(body)).is_err() {
            let error = Error::new(
                "session_disconnected",
                "Original effects may be unknown; never replay.",
            );
            *record.reply.lock().unwrap() = Some(Err(error.clone()));
            record.notify.notify_waiters();
            state.broken = Some(error);
            return Err(Error::new(
                "session_disconnected",
                "Original effects may be unknown; never replay.",
            ));
        }
        Ok(record)
    }
    async fn wait(record: Arc<Record>) -> Result<Reply> {
        loop {
            let notified = record.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(reply) = record.reply.lock().unwrap().clone() {
                return reply;
            }
            notified.await;
        }
    }
    async fn request(
        &self,
        channel: &str,
        op: &str,
        args: Value,
        id: Option<&str>,
    ) -> Result<Reply> {
        let record = self.submit(channel, op, args, id)?;
        let mut guard = CancelGuard {
            transport: self,
            armed: true,
        };
        let result = Self::wait(record).await;
        guard.armed = false;
        result
    }
    async fn result(
        &self,
        channel: &str,
        op: &str,
        args: Value,
        id: Option<&str>,
    ) -> Result<Value> {
        let reply = self.request(channel, op, args, id).await?;
        if let Some(fault) = &reply.error {
            return Err(Error {
                code: fault.code.clone(),
                message: fault.message.clone(),
                reply: Some(reply),
            });
        }
        Ok(reply.result.unwrap_or(Value::Null))
    }
}
struct CancelGuard<'a> {
    transport: &'a Transport,
    armed: bool,
}
impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.transport.submit("host", "end_turn", json!({}), None);
        }
    }
}
fn fail(state: &Arc<Mutex<State>>, error: Error) {
    let mut state = state.lock().unwrap();
    state.broken = Some(error.clone());
    for record in state.records.values() {
        let mut reply = record.reply.lock().unwrap();
        if reply.is_none() {
            *reply = Some(Err(error.clone()));
            record.notify.notify_waiters();
        }
    }
}
#[derive(Clone)]
pub struct DesktopClient {
    transport: Arc<Transport>,
}
impl DesktopClient {
    pub async fn call(&self, op: &str, args: Value, id: Option<&str>) -> Result<Value> {
        if !["observe", "read", "sync", "act", "capture", "get", "cancel"].contains(&op) {
            return Err(Error::new(
                "invalid_argument",
                "Authorization belongs to HostSession.",
            ));
        }
        self.transport.result("desktop", op, args, id).await
    }
    pub async fn observe(&self, args: Value) -> Result<Observation> {
        let mut request = json!({"scope":{"desktop":true},"projection":"summary","fields":["name","role","app","window"],"budget":{"max_results":32,"max_output_bytes":8192}});
        if let Some(object) = args.as_object() {
            for (k, v) in object {
                request[k] = v.clone();
            }
        }
        let mut ob: Observation = decode(self.call("observe", request.clone(), None).await?)?;
        ob.query = request;
        Ok(ob)
    }
    pub async fn outline(&self, reference: &str) -> Result<Observation> {
        self.observe(json!({"scope":{"refs":[reference]},"projection":"outline","fields":["name","role"],"budget":{"max_depth":4,"max_results":32,"max_output_bytes":8192}})).await
    }
    pub async fn next(&self, ob: &Observation) -> Result<Observation> {
        let cursor =
            ob.coverage.continuation.as_ref().ok_or_else(|| {
                Error::new("invalid_argument", "Observation has no continuation.")
            })?;
        if !ob.query.is_object() {
            return Err(Error::new(
                "invalid_argument",
                "Use the originating observation to preserve its query.",
            ));
        }
        let mut request = ob.query.clone();
        request["continuation"] = json!(cursor);
        let mut next: Observation = decode(self.call("observe", request.clone(), None).await?)?;
        next.query = request;
        Ok(next)
    }
    pub async fn find(&self, within: &str, mut locator: Value) -> Result<Observation> {
        if !locator.is_object() {
            return Err(Error::new(
                "invalid_argument",
                "Locator requires an object.",
            ));
        }
        locator["within"] = json!(within);
        self.observe(json!({"scope":{"refs":[within]},"projection":"outline","fields":["name","role"],"match":locator,"budget":{"max_depth":12,"max_results":32,"max_output_bytes":8192}})).await
    }
    pub async fn act(&self, plan: &Plan, id: Option<&str>) -> Result<Receipt> {
        if plan.steps.is_empty() || plan.steps.len() > 16 {
            return Err(Error::new(
                "invalid_argument",
                "A native plan has 1..16 steps.",
            ));
        }
        let reply = self
            .transport
            .request("desktop", "act", json!(plan), id)
            .await?;
        if let Some(fault) = &reply.error {
            return Err(Error {
                code: fault.code.clone(),
                message: fault.message.clone(),
                reply: Some(reply),
            });
        }
        let receipt: Receipt = decode(reply.result.clone().unwrap_or(Value::Null))?;
        if receipt.outcome != "completed" {
            return Err(Error {
                code: "action_not_completed".into(),
                message: "Inspect original receipt; do not replay.".into(),
                reply: Some(reply),
            });
        }
        Ok(receipt)
    }
    pub async fn read(&self, reference: &str) -> Result<Value> {
        self.call("read", json!({"target":reference}), None).await
    }
    pub async fn sync(&self, cursor: &str) -> Result<Value> {
        self.call("sync", json!({"cursor":cursor}), None).await
    }
    pub async fn capture(&self, args: Value) -> Result<Value> {
        self.call("capture", args, None).await
    }
    pub async fn get(&self, run_id: &str) -> Result<Receipt> {
        decode(self.call("get", json!({"run_id":run_id}), None).await?)
    }
    pub async fn cancel(&self, run_id: &str) -> Result<Receipt> {
        decode(self.call("cancel", json!({"run_id":run_id}), None).await?)
    }
    pub async fn reconcile(&self, id: &str) -> Result<Reply> {
        let record = self
            .transport
            .state
            .lock()
            .unwrap()
            .records
            .get(id)
            .cloned()
            .ok_or_else(|| Error::new("invalid_argument", "No original request with this ID."))?;
        Transport::wait(record).await
    }
}
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|e| Error::new("invalid_reply", e.to_string()))
}
pub struct HostSession {
    transport: Arc<Transport>,
    pub desktop: DesktopClient,
    pub hello: Value,
}
impl HostSession {
    pub async fn start(options: SessionOptions) -> Result<Self> {
        let mut command = Command::new(options.helper);
        command
            .arg("session")
            .arg("--input-mode")
            .arg(options.input_mode)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);
        if let Some(policy) = options.input_policy {
            command.arg("--input-policy").arg(policy);
        }
        if let Some(mode) = options.audit_mode {
            command.arg("--audit-mode").arg(mode);
        }
        for name in options.write_apps {
            command.arg("--write-app").arg(name);
        }
        for title in options.write_app_windows {
            command.arg("--write-app-window").arg(title);
        }
        for (flag, path) in [
            ("--assets-dir", options.assets_dir),
            ("--audit", options.audit),
            ("--session", options.owner_file),
        ] {
            if let Some(path) = path {
                command.arg(flag).arg(path);
            }
        }
        let mut child = command
            .spawn()
            .map_err(|e| Error::new("startup_failed", e.to_string()))?;
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let line = tokio::time::timeout(Duration::from_secs(15), lines.next_line())
            .await
            .map_err(|_| Error::new("startup_timeout", "Session did not become ready."))?
            .map_err(|e| Error::new("startup_failed", e.to_string()))?
            .ok_or_else(|| Error::new("startup_failed", "Missing session hello."))?;
        let hello: Value =
            serde_json::from_str(&line).map_err(|e| Error::new("startup_failed", e.to_string()))?;
        if hello["protocol"] != "desktop-world/session-v0.1"
            || !hello["features"]
                .as_array()
                .is_some_and(|a| a.contains(&json!("dynamic_app_grants")))
        {
            return Err(Error::new(
                "incompatible_helper",
                "Use the matching RC helper.",
            ));
        }
        let mut stdin = child.stdin.take().unwrap();
        let state = Arc::new(Mutex::new(State {
            records: HashMap::new(),
            broken: None,
        }));
        let (writer, mut receiver) = mpsc::unbounded_channel();
        let writer_state = state.clone();
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                match message {
                    Write::Frame(body) => {
                        if let Err(e) = stdin.write_all(&body).await {
                            fail(
                                &writer_state,
                                Error::new("transport_unknown", e.to_string()),
                            );
                            break;
                        }
                    }
                    Write::Close => break,
                }
            }
            let _ = stdin.shutdown().await;
        });
        let reader_state = state.clone();
        let reader_writer = writer.clone();
        tokio::spawn(async move {
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        if line.len() > 2 * 1024 * 1024 {
                            fail(
                                &reader_state,
                                Error::new("invalid_reply", "Session frame too large."),
                            );
                            break;
                        }
                        match serde_json::from_str::<Reply>(&line) {
                            Ok(reply) => {
                                let record =
                                    reader_state.lock().unwrap().records.get(&reply.id).cloned();
                                if let Some(record) = record {
                                    *record.reply.lock().unwrap() = Some(Ok(reply));
                                    record.notify.notify_waiters();
                                } else {
                                    fail(
                                        &reader_state,
                                        Error::new(
                                            "invalid_reply",
                                            "Unexpected session reply; effects may be unknown.",
                                        ),
                                    );
                                    break;
                                }
                            }
                            Err(e) => {
                                fail(&reader_state, Error::new("invalid_reply", e.to_string()));
                                break;
                            }
                        }
                    }
                    _ => {
                        fail(
                            &reader_state,
                            Error::new(
                                "session_disconnected",
                                "Session exited; preserve receipts and never replay.",
                            ),
                        );
                        break;
                    }
                }
            }
            let _ = reader_writer.send(Write::Close);
        });
        let transport = Arc::new(Transport {
            state,
            writer,
            sequence: AtomicU64::new(1),
            child: tokio::sync::Mutex::new(child),
        });
        Ok(Self {
            desktop: DesktopClient {
                transport: transport.clone(),
            },
            transport,
            hello,
        })
    }
    pub async fn grant(&self, application: &str) -> Result<Value> {
        self.transport
            .result("host", "grant", json!({"application":application}), None)
            .await
    }
    pub async fn declare(&self, name: &str) -> Result<Value> {
        self.transport
            .result("host", "declare", json!({"name":name}), None)
            .await
    }
    pub async fn declare_window(&self, title: &str) -> Result<Value> {
        self.transport
            .result("host", "declare", json!({"window_title":title}), None)
            .await
    }
    pub async fn revoke(&self, application: &str) -> Result<Value> {
        self.transport
            .result("host", "revoke", json!({"application":application}), None)
            .await
    }
    pub async fn revoke_grant(&self, id: &str) -> Result<Value> {
        self.transport
            .result("host", "revoke", json!({"grant_id":id}), None)
            .await
    }
    pub async fn grants(&self) -> Result<GrantStatus> {
        decode(
            self.transport
                .result("host", "grants", json!({}), None)
                .await?,
        )
    }
    pub async fn begin_turn(&self, turn: &str) -> Result<Value> {
        self.transport
            .result("host", "begin_turn", json!({"turn":turn}), None)
            .await
    }
    pub async fn end_turn(&self) -> Result<Value> {
        self.transport
            .result("host", "end_turn", json!({}), None)
            .await
    }
    pub async fn close(&self) -> Result<()> {
        let _ = self.transport.writer.send(Write::Close);
        let mut child = self.transport.child.lock().await;
        match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
            Ok(Ok(status)) if status.success() => Ok(()),
            Ok(Ok(_)) => Err(Error::new(
                "close_incomplete",
                "Session exited unsuccessfully; native cleanup is not confirmed.",
            )),
            Ok(Err(e)) => Err(Error::new("close_failed", e.to_string())),
            Err(_) => {
                child
                    .kill()
                    .await
                    .map_err(|e| Error::new("close_incomplete", e.to_string()))?;
                Err(Error::new(
                    "close_incomplete",
                    "Forced process termination does not prove native cleanup.",
                ))
            }
        }
    }
}
impl Drop for HostSession {
    fn drop(&mut self) {
        let _ = self.transport.writer.send(Write::Close);
    }
}

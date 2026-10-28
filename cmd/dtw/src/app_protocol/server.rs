//! Standard-tool orchestration. Selection is explicit; approval belongs to the MCP client.
use super::{
    app_id, problem, scripts,
    wire::{self, Command, Request, Submit, Target, WireError},
    Coordinator, Reply,
};
use crate::computer::{AppInstance, Backend, Error};
use serde_json::{json, Value};
use std::{
    collections::{HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

// A native worker retains its scene lease while its capture client is alive.
// Switching apps must finish retiring that worker before another can connect.
const MAX_CONTEXTS: usize = 1;
const MAX_RECORDS: usize = 512;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
// Receipts and step inventory evidence cannot be evicted like images/results.
// Stop admitting new work at this threshold, preserving the last complete receipt.
const MAX_RECEIPT_BYTES: usize = 16 * 1024 * 1024;
const MAX_SHUTDOWN_ERRORS: usize = 8;
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct Application {
    pub app_id: String,
    pub app: AppInstance,
    pub name: String,
}
pub trait Factory: Send + Sync + 'static {
    type Provider: Backend + 'static;
    fn catalog(&self) -> Result<Vec<Application>, Error>;
    fn connect(&self) -> Result<Self::Provider, Error>;
    fn description(&self) -> Value {
        json!({"coverage":"Provider-defined application window subset","conditions":[]})
    }
    /// Stops a possibly blocked metadata read. A later call may create a new reader.
    fn close_catalog(&self) -> Result<(), Error>;
}
struct Record {
    id: String,
    app: String,
    input: Value,
    reply: Option<Reply>,
    payload_bytes: usize,
    receipt_bytes: usize,
}
struct AppContext<B: Backend> {
    app_id: String,
    owner: Arc<Coordinator<B>>,
}
struct State<B: Backend> {
    // Includes preparing and failed-to-retire owners until cleanup succeeds.
    contexts: VecDeque<AppContext<B>>,
    retired_apps: HashSet<String>,
    records: VecDeque<Record>,
    active: Option<Arc<Call<B>>>,
    closed: bool,
    shutdown_errors: Vec<WireError>,
}
pub struct Server<F: Factory> {
    factory: F,
    state: Mutex<State<F::Provider>>,
    finished: Condvar,
}
pub struct Call<B: Backend> {
    pub rpc_id: Value,
    pub cancelled: AtomicBool,
    owner: Mutex<Option<Arc<Coordinator<B>>>>,
    request_id: Option<String>,
    command: ToolCall,
    run_finished: AtomicBool,
    context_reset: AtomicBool,
}
#[derive(Clone)]
enum ToolCall {
    Apps,
    Exec {
        app: String,
        id: String,
        source: String,
        timeout_ms: u64,
        max_calls: u32,
    },
}
pub enum Started<B: Backend> {
    Reply(Reply),
    Call(Arc<Call<B>>),
}

impl<F: Factory> Server<F> {
    pub fn new(factory: F) -> Arc<Self> {
        Arc::new(Self {
            factory,
            state: Mutex::new(State {
                contexts: VecDeque::new(),
                retired_apps: HashSet::new(),
                records: VecDeque::new(),
                active: None,
                closed: false,
                shutdown_errors: Vec::new(),
            }),
            finished: Condvar::new(),
        })
    }
    /// Reserve before spawning. Neither cancellation nor a burst can overtake admission.
    pub fn begin(&self, rpc_id: Value, name: &str, args: Value) -> Started<F::Provider> {
        if name == "result" {
            return Started::Reply(self.result(args));
        }
        let command = match parse_call(name, &args) {
            Ok(c) => c,
            Err(e) => return Started::Reply(Reply::error(e)),
        };
        let mut state = self.state.lock().unwrap();
        self.refresh(&mut state);
        if state.active.as_ref().is_some_and(|c| c.rpc_id == rpc_id) {
            return Started::Reply(problem(
                "duplicate_rpc_id",
                "The MCP request ID is already active",
            ));
        }
        if let ToolCall::Exec { app, id, .. } = &command {
            if let Some(record) = state.records.iter().find(|r| r.id == *id) {
                if record.app != *app || record.input != args {
                    return Started::Reply(problem("request_conflict", "This request_id already belongs to different input; it will not execute again"));
                }
                return Started::Reply(self.record_reply(&state, record));
            }
        }
        if !state.shutdown_errors.is_empty() {
            return Started::Reply(shutdown_reply(&state.shutdown_errors));
        }
        if state.closed {
            return Started::Reply(problem("closed", "The MCP server is shutting down"));
        }
        if state.active.is_some() {
            return Started::Reply(problem(
                "busy",
                "Another desktop call is active or has not settled; no operation was queued",
            ));
        }
        let request_id = match &command {
            ToolCall::Apps => None,
            ToolCall::Exec { app, id, .. } => {
                if state.records.len() >= MAX_RECORDS
                    || state.records.iter().map(|r| r.receipt_bytes).sum::<usize>()
                        >= MAX_RECEIPT_BYTES
                {
                    return Started::Reply(problem("request_limit", "Retained receipt capacity is exhausted. Reconnect for new work; do not replay uncertain operations"));
                }
                state.records.push_back(Record {
                    id: id.clone(),
                    app: app.clone(),
                    input: args,
                    reply: None,
                    payload_bytes: 0,
                    receipt_bytes: 0,
                });
                Some(id.clone())
            }
        };
        let call = Arc::new(Call {
            rpc_id,
            cancelled: AtomicBool::new(false),
            owner: Mutex::new(None),
            request_id,
            command,
            run_finished: AtomicBool::new(false),
            context_reset: AtomicBool::new(false),
        });
        state.active = Some(call.clone());
        Started::Call(call)
    }
    pub fn run(&self, call: &Arc<Call<F::Provider>>) -> Reply {
        let reply = match &call.command {
            ToolCall::Apps => self.apps(call),
            ToolCall::Exec {
                app,
                id,
                source,
                timeout_ms,
                max_calls,
            } => self.execute(call, app, id, source, *timeout_ms, *max_calls),
        };
        let stopping = call.cancelled.load(Ordering::Acquire) || self.state.lock().unwrap().closed;
        if stopping {
            // Cancellation may have closed the reader immediately before a late
            // catalog call created it. Reap again before announcing run completion.
            if let Err(error) = self.factory.close_catalog() {
                self.remember_shutdown(wire::core_error(&error));
            }
        }
        self.finish_run(call, reply)
    }
    fn finish_run(&self, call: &Arc<Call<F::Provider>>, mut reply: Reply) -> Reply {
        let mut state = self.state.lock().unwrap();
        let owner = call.owner.lock().unwrap().clone();
        let unsettled = owner
            .as_ref()
            .is_some_and(|owner| inspect_owner(&mut state, owner));
        if !unsettled {
            if let Some((owner, id)) = owner.as_ref().zip(call.request_id.as_ref()) {
                let current = owner.result(id);
                if current.data["error"]["code"] != "unknown_request" {
                    // The job may have completed after execute took a running
                    // snapshot. Never freeze that stale snapshot as terminal.
                    reply = current;
                }
            }
        }
        if call.request_id.is_some() && !unsettled {
            // Accepted requests rejected before Core submission still have a
            // terminal record (for example cancellation during discovery).
            reply.data["status"] = json!("completed");
        }
        self.annotate(&state, call, &mut reply);
        // A timeout of the outer wait is not a terminal native receipt. Keep the
        // owner and global reservation until its actual job has finished.
        if !unsettled {
            if let Some(id) = &call.request_id {
                cache_reply(&mut state, id, &reply);
            }
            if state
                .active
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, call))
            {
                state.active = None;
            }
        }
        call.run_finished.store(true, Ordering::Release);
        self.finished.notify_all();
        reply
    }
    fn refresh(&self, state: &mut State<F::Provider>) {
        let Some(call) = state
            .active
            .clone()
            .filter(|call| call.run_finished.load(Ordering::Acquire))
        else {
            return;
        };
        let owner = call.owner.lock().unwrap().clone();
        if let Some(owner) = owner {
            if inspect_owner(state, &owner) {
                return;
            }
            if let Some(id) = &call.request_id {
                let mut reply = owner.result(id);
                self.annotate(state, &call, &mut reply);
                cache_reply(state, id, &reply);
            }
        }
        state.active = None;
    }
    fn annotate(&self, state: &State<F::Provider>, call: &Call<F::Provider>, reply: &mut Reply) {
        if let ToolCall::Exec { app, id, .. } = &call.command {
            reply.data["app"] = json!(app);
            reply.data["request_id"] = json!(id);
            reply.data["context_reset"] = json!(call.context_reset.load(Ordering::Acquire));
        }
        append_shutdown_errors(reply, &state.shutdown_errors);
    }
    fn apps(&self, call: &Call<F::Provider>) -> Reply {
        if call.cancelled.load(Ordering::Acquire) {
            return problem("cancelled", "Application discovery was cancelled");
        }
        match self.factory.catalog() {
            Ok(apps) if !call.cancelled.load(Ordering::Acquire) => {
                let apps = public_apps(apps);
                Reply::plain(json!({"version":1,
                "apps":apps,"backend":self.factory.description(),
                "script":scripts::description(),
                "limits":{"resident_app_contexts":MAX_CONTEXTS,"retained_request_ids":MAX_RECORDS,"payload_bytes":MAX_PAYLOAD_BYTES,
                    "receipt_admission_bytes":MAX_RECEIPT_BYTES,"max_steps_per_request":scripts::MAX_CALLS},
                "semantics":"Every exec requires a stable application identifier. Multiple running instances of that application must be resolved before execution. DTW validates identity and confines execution. State and references belong to one resident app context; cancellation, timeout or cache replacement clears them. Results preserve their original app and request_id. Receipt capacity stops new work without dropping the last receipt."}))
            }
            Ok(_) => problem("cancelled", "Application discovery was cancelled"),
            Err(e) => Reply::error(wire::core_error(&e)),
        }
    }
    fn execute(
        &self,
        call: &Call<F::Provider>,
        app_id: &str,
        id: &str,
        source: &str,
        timeout_ms: u64,
        max_calls: u32,
    ) -> Reply {
        if call.cancelled.load(Ordering::Acquire) {
            return problem("cancelled", "Cancelled before script admission");
        }
        let entry = match self.factory.catalog() {
            Ok(apps) => {
                let mut matches = apps.into_iter().filter(|entry| entry.app_id == app_id);
                let Some(entry) = matches.next() else {
                    return problem(
                        "app_unavailable",
                        "The application is not running. Discover again; no script was dispatched",
                    );
                };
                if matches.next().is_some() {
                    return problem("ambiguous_app", "More than one running instance has this application identifier; no script was dispatched");
                }
                entry
            }
            Err(e) => return Reply::error(wire::core_error(&e)),
        };
        if call.cancelled.load(Ordering::Acquire) {
            return problem("cancelled", "Cancelled before script admission");
        }
        let app = &entry.app;
        let candidate = self
            .state
            .lock()
            .unwrap()
            .contexts
            .iter()
            .find(|context| context.app_id == app_id)
            .map(|context| context.owner.clone());
        let existing = match candidate {
            Some(owner) => {
                let reusable = {
                    let state = owner.state.lock().unwrap();
                    owner.app == *app
                        && state.terminal.is_none()
                        && state.jobs.len() < owner.max_requests
                };
                if reusable {
                    Some(owner)
                } else {
                    if let Err(reply) = self.retire(&owner) {
                        return reply;
                    }
                    call.context_reset.store(true, Ordering::Release);
                    None
                }
            }
            None => {
                if self.state.lock().unwrap().retired_apps.contains(app_id) {
                    call.context_reset.store(true, Ordering::Release);
                }
                None
            }
        };
        let owner = if let Some(owner) = existing {
            owner
        } else {
            // Free capacity before connecting; a failed retirement remains owned
            // and closes new admission instead of being replaced by another worker.
            let evicted = {
                let state = self.state.lock().unwrap();
                if !state.shutdown_errors.is_empty() {
                    return shutdown_reply(&state.shutdown_errors);
                }
                if state.closed || call.cancelled.load(Ordering::Acquire) {
                    return problem("cancelled", "Stopped before creating an app context");
                }
                (state.contexts.len() >= MAX_CONTEXTS)
                    .then(|| state.contexts.front().unwrap().owner.clone())
            };
            if let Some(evicted) = evicted {
                if let Err(reply) = self.retire(&evicted) {
                    return reply;
                }
            }
            let backend = match self.factory.connect() {
                Ok(backend) => backend,
                Err(error) => return Reply::error(wire::core_error(&error)),
            };
            let mut owner = Coordinator::new(backend, app.clone(), entry.name);
            Arc::get_mut(&mut owner).unwrap().max_payload_bytes = MAX_PAYLOAD_BYTES / MAX_CONTEXTS;
            // Register even if close/cancel won during connect. close() waits for
            // this run and performs a second pass over all registered owners.
            self.state.lock().unwrap().contexts.push_back(AppContext {
                app_id: app_id.into(),
                owner: owner.clone(),
            });
            owner
        };
        let admitted = {
            // Lock ordering is always Server state -> call owner -> Coordinator.
            let state = self.state.lock().unwrap();
            let mut installed = call.owner.lock().unwrap();
            *installed = Some(owner.clone());
            let stopped = state.closed
                || !state.shutdown_errors.is_empty()
                || call.cancelled.load(Ordering::Acquire);
            drop(state);
            if stopped {
                drop(installed);
                let closed = owner.close();
                if let Some(error) = cleanup_error(&closed) {
                    self.remember_shutdown(error);
                    return closed;
                }
                return problem("cancelled", "Stopped before script admission");
            }
            owner.handle(Request::Submit(Submit {
                request_id: id.into(),
                target: Target {
                    app_ref: owner.app_ref.clone(),
                    window_ref: None,
                },
                command: Command::Script {
                    source: source.into(),
                    timeout_ms,
                    max_calls,
                },
            }))
        };
        if admitted.data["status"] != "running" {
            return admitted;
        }
        if !owner.wait_idle(Duration::from_millis(timeout_ms) + Duration::from_secs(3)) {
            let cancelled = owner.cancel(id);
            if let Some(error) = cleanup_error(&cancelled) {
                self.remember_shutdown(error);
            }
            if !owner.wait_idle(STOP_TIMEOUT) {
                let mut reply = owner.result(id);
                reply.data["execution_unsettled"] = json!(true);
                return reply;
            }
        }
        owner.result(id)
    }
    fn retire(&self, owner: &Arc<Coordinator<F::Provider>>) -> Result<(), Reply> {
        let reply = owner.close();
        if let Some(error) = cleanup_error(&reply) {
            self.remember_shutdown(error);
            return Err(reply);
        }
        if !owner.wait_idle(Duration::ZERO) {
            let error = shutdown_error("Retiring app context has not settled");
            self.remember_shutdown(error.clone());
            return Err(Reply::error(error));
        }
        let mut state = self.state.lock().unwrap();
        if let Some(index) = state
            .contexts
            .iter()
            .position(|entry| Arc::ptr_eq(&entry.owner, owner))
        {
            let retired = state.contexts.remove(index).unwrap();
            // At most one ID per accepted exec; bounded by MAX_RECORDS.
            state.retired_apps.insert(retired.app_id);
        }
        Ok(())
    }
    fn remember_shutdown(&self, error: WireError) {
        remember_shutdown(&mut self.state.lock().unwrap(), error);
    }
    fn record_reply(&self, state: &State<F::Provider>, record: &Record) -> Reply {
        let mut reply = if let Some(reply) = &record.reply {
            reply.clone()
        } else {
            state
                .active
                .as_ref()
                .filter(|active| active.request_id.as_deref() == Some(&record.id))
                .and_then(|active| {
                    active
                        .owner
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|owner| owner.result(&record.id))
                })
                .filter(|reply| reply.data["error"]["code"] != "unknown_request")
                .unwrap_or_else(|| Reply::plain(json!({"version":1,"status":"running"})))
        };
        reply.data["app"] = json!(record.app);
        reply.data["request_id"] = json!(record.id);
        append_shutdown_errors(&mut reply, &state.shutdown_errors);
        reply
    }
    fn result(&self, args: Value) -> Reply {
        let parsed = (|| {
            exact_object(&args, &["app", "request_id"])?;
            Ok::<_, WireError>((app_id::parse(&args["app"])?, request_id(&args)?))
        })();
        let (app, id) = match parsed {
            Ok(value) => value,
            Err(error) => return Reply::error(error),
        };
        let mut state = self.state.lock().unwrap();
        self.refresh(&mut state);
        match state.records.iter().find(|record| record.id == id) {
            Some(record) if record.app == app => self.record_reply(&state, record),
            Some(_) => problem("wrong_app", "The request belongs to a different app identifier"),
            None => problem("unknown_request", "No retained request has this ID. Absence does not prove an operation had no effect"),
        }
    }
    pub fn cancel(&self, rpc_id: &Value) {
        let active = self.state.lock().unwrap().active.clone();
        if let Some(call) = active.filter(|call| call.rpc_id == *rpc_id) {
            call.cancelled.store(true, Ordering::Release);
            let cancelled = {
                let owner = call.owner.lock().unwrap();
                owner
                    .as_ref()
                    .zip(call.request_id.as_ref())
                    .map(|(owner, id)| owner.cancel(id))
            };
            if let Some(error) = cancelled.as_ref().and_then(cleanup_error) {
                self.remember_shutdown(error);
            }
            if let Err(error) = self.factory.close_catalog() {
                self.remember_shutdown(wire::core_error(&error));
            }
        }
    }
    pub fn close(&self) -> Result<(), WireError> {
        let active = {
            let mut state = self.state.lock().unwrap();
            state.closed = true;
            state.active.clone()
        };
        if let Some(call) = &active {
            call.cancelled.store(true, Ordering::Release);
        }
        if let Err(error) = self.factory.close_catalog() {
            self.remember_shutdown(wire::core_error(&error));
        }
        self.close_contexts();
        if let Some(call) = active {
            let until = Instant::now() + STOP_TIMEOUT;
            let mut state = self.state.lock().unwrap();
            while !call.run_finished.load(Ordering::Acquire) {
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                state = self.finished.wait_timeout(state, left).unwrap().0;
            }
            let unfinished = !call.run_finished.load(Ordering::Acquire);
            drop(state);
            if unfinished {
                self.remember_shutdown(shutdown_error(
                    "Preparing or running tool call did not settle",
                ));
            }
        }
        self.close_contexts();
        let mut state = self.state.lock().unwrap();
        self.refresh(&mut state);
        if state.shutdown_errors.is_empty() {
            Ok(())
        } else {
            Err(WireError {
                code: "shutdown_failed".into(),
                message: "Native cleanup did not complete cleanly; original errors are retained"
                    .into(),
                details: Some(
                    json!({"errors": state.shutdown_errors.iter().map(super::error_json).collect::<Vec<_>>()}),
                ),
            })
        }
    }
    fn close_contexts(&self) {
        let owners: Vec<_> = self
            .state
            .lock()
            .unwrap()
            .contexts
            .iter()
            .map(|context| context.owner.clone())
            .collect();
        for owner in owners {
            let reply = owner.close();
            if let Some(error) = cleanup_error(&reply) {
                self.remember_shutdown(error);
            }
            if !owner.wait_idle(STOP_TIMEOUT) {
                self.remember_shutdown(shutdown_error("Active script did not settle"));
            }
        }
    }
}
// These helpers run under the Server lock. Coordinator never calls back into
// Server, preserving the state -> owner -> Coordinator lock order.
fn inspect_owner<B: Backend>(state: &mut State<B>, owner: &Coordinator<B>) -> bool {
    let owned = owner.state.lock().unwrap();
    if let Some(error) = &owned.shutdown_error {
        remember_shutdown(state, error.clone());
    }
    owned.active.is_some()
}
fn remember_shutdown<B: Backend>(state: &mut State<B>, error: WireError) {
    if state.shutdown_errors.contains(&error) {
        return;
    }
    if state.shutdown_errors.len() < MAX_SHUTDOWN_ERRORS {
        state.shutdown_errors.push(error);
    } else {
        let last = state.shutdown_errors.last_mut().unwrap();
        let omitted = if last.code == "additional_shutdown_failures" {
            last.details
                .as_ref()
                .and_then(|details| details["omitted"].as_u64())
                .unwrap_or(1)
                .saturating_add(1)
        } else {
            2
        };
        *last = WireError {
            code: "additional_shutdown_failures".into(),
            message: "Additional cleanup errors exceeded the diagnostic sample limit".into(),
            details: Some(json!({"omitted":omitted})),
        };
    }
}
fn cache_reply<B: Backend>(state: &mut State<B>, id: &str, reply: &Reply) {
    if let Some(record) = state.records.iter_mut().find(|record| record.id == id) {
        record.payload_bytes = payload_size(reply);
        record.receipt_bytes = receipt_size(reply);
        record.reply = Some(reply.clone());
    }
    let mut bytes: usize = state
        .records
        .iter()
        .map(|record| record.payload_bytes)
        .sum();
    for record in &mut state.records {
        if bytes <= MAX_PAYLOAD_BYTES {
            break;
        }
        bytes -= record.payload_bytes;
        record.payload_bytes = 0;
        if let Some(reply) = &mut record.reply {
            reply.image = None;
            reply.data["value"] = Value::Null;
            reply.data["payload_expired"] = json!(true);
            record.receipt_bytes = receipt_size(reply);
        }
    }
}
fn payload_size(reply: &Reply) -> usize {
    reply
        .data
        .get("value")
        .map_or(0, |value| value.to_string().len())
        + reply.image.as_ref().map_or(0, |image| image.len())
}
fn receipt_size(reply: &Reply) -> usize {
    reply.data.to_string().len().saturating_sub(
        reply
            .data
            .get("value")
            .map_or(0, |value| value.to_string().len()),
    )
}
fn shutdown_error(message: &str) -> WireError {
    WireError {
        code: "shutdown_failed".into(),
        message: message.into(),
        details: None,
    }
}
fn cleanup_error(reply: &Reply) -> Option<WireError> {
    if !reply.is_error {
        return None;
    }
    let value = if reply.data["shutdown_error"].is_object() {
        &reply.data["shutdown_error"]
    } else {
        &reply.data["error"]
    };
    Some(WireError {
        code: value["code"].as_str().unwrap_or("shutdown_failed").into(),
        message: value["message"]
            .as_str()
            .unwrap_or("Native cleanup failed")
            .into(),
        details: value.get("details").cloned(),
    })
}
fn append_shutdown_errors(reply: &mut Reply, errors: &[WireError]) {
    if !errors.is_empty() {
        reply.data["shutdown_errors"] =
            json!(errors.iter().map(super::error_json).collect::<Vec<_>>());
        reply.is_error = true;
    }
}
fn shutdown_reply(errors: &[WireError]) -> Reply {
    let mut reply = problem(
        "shutdown_failed",
        "Native cleanup failed; new execution is closed. Inspect original request results",
    );
    append_shutdown_errors(&mut reply, errors);
    reply
}
fn invalid(message: &str) -> WireError {
    WireError {
        code: "invalid_arguments".into(),
        message: message.into(),
        details: None,
    }
}
fn exact_object<'a>(
    value: &'a Value,
    keys: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, WireError> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("Tool arguments must be an object"))?;
    if map.keys().any(|k| !keys.contains(&k.as_str())) {
        return Err(invalid("Unknown tool argument"));
    }
    Ok(map)
}
fn request_id(args: &Value) -> Result<String, WireError> {
    args["request_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= 128)
        .map(str::to_owned)
        .ok_or_else(|| invalid("request_id must contain 1 to 128 characters"))
}
fn parse_call(name: &str, args: &Value) -> Result<ToolCall, WireError> {
    match name {
        "apps" => {
            exact_object(args, &[])?;
            Ok(ToolCall::Apps)
        }
        "exec" => {
            exact_object(
                args,
                &["app", "request_id", "source", "timeout_ms", "max_calls"],
            )?;
            let app = app_id::parse(&args["app"])?;
            let id = request_id(args)?;
            let source = args["source"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= scripts::MAX_SOURCE_BYTES)
                .ok_or_else(|| {
                    invalid("source must be a nonempty JavaScript function body of at most 64 KiB")
                })?
                .to_owned();
            let number = |key: &str, default: u64| {
                match args.get(key) { None=>Ok(default),Some(v)=>v.as_u64().filter(|n|*n>0&&*n<=default).ok_or_else(||invalid("Execution limits must be positive integers within the advertised bounds"))}
            };
            Ok(ToolCall::Exec {
                app,
                id,
                source,
                timeout_ms: number("timeout_ms", scripts::MAX_TIMEOUT_MS)?,
                max_calls: number("max_calls", scripts::MAX_CALLS as u64)? as u32,
            })
        }
        _ => Err(invalid("Unknown tool")),
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::computer::*;
    use std::sync::{atomic::AtomicUsize, mpsc};

    struct Gate {
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    struct Control {
        entered: mpsc::Receiver<()>,
        release: mpsc::SyncSender<()>,
    }
    fn gate() -> (Arc<Gate>, Control) {
        let (entered, observed) = mpsc::sync_channel(0);
        let (release, released) = mpsc::sync_channel(0);
        (
            Arc::new(Gate {
                entered,
                release: Mutex::new(released),
            }),
            Control {
                entered: observed,
                release,
            },
        )
    }
    impl Gate {
        fn enter(&self) {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .expect("test did not release checkpoint");
        }
    }
    impl Control {
        fn entered(&self) {
            self.entered
                .recv_timeout(Duration::from_secs(5))
                .expect("checkpoint not reached");
        }
        fn release(&self) {
            self.release.send(()).unwrap();
        }
    }
    #[derive(Default)]
    struct Probe {
        press: Mutex<Option<Arc<Gate>>>,
        shutdowns: AtomicUsize,
        actions: AtomicUsize,
        shutdown_fails: AtomicBool,
    }
    #[derive(Clone)]
    struct Provider(Arc<Probe>);
    impl Backend for Provider {
        type Binding = ();
        fn stamp(&self) -> ProviderStamp {
            ProviderStamp {
                id: "server-test".into(),
                generation: 1,
            }
        }
        fn supports(&self, capability: Capability) -> bool {
            matches!(capability, Capability::Observe | Capability::Press)
        }
        fn shutdown(&self) -> Result<(), Error> {
            self.0.shutdowns.fetch_add(1, Ordering::SeqCst);
            if self.0.shutdown_fails.load(Ordering::SeqCst) {
                Err(Error::NativeFailure {
                    code: "drain_failed".into(),
                    message: "original drain message".into(),
                })
            } else {
                Ok(())
            }
        }
        fn windows(
            &self,
            app: &AppInstance,
            _: &CallContext<'_>,
        ) -> Result<WindowInventory, Error> {
            Ok(test_inventory(vec![WindowInfo {
                target: WindowTarget {
                    app: app.clone(),
                    native_id: 1,
                    generation: 1,
                },
                title: "Window".into(),
                visible: true,
            }]))
        }
        fn observe(
            &self,
            target: &WindowTarget,
            _: &CallContext<'_>,
        ) -> Result<BackendObservation<()>, Error> {
            Ok(BackendObservation {
                target: target.clone(),
                binding: (),
                image: None,
                elements: vec![Element {
                    id: ElementId(1),
                    label: "Action".into(),
                    role: "button".into(),
                    name: Some("Action".into()),
                    value: None,
                    enabled: Some(true),
                    actions: vec!["press".into()],
                    can_press: true,
                }],
            })
        }
        fn press(
            &self,
            _: &WindowTarget,
            _: &(),
            _: ElementId,
            _: &CallContext<'_>,
        ) -> ActionOutcome {
            let gate = self.0.press.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.enter();
            }
            self.0.actions.fetch_add(1, Ordering::SeqCst);
            ActionOutcome {
                dispatch: DispatchState::Dispatched,
                evidence: EffectEvidence::Unverified,
                error: None,
            }
        }
    }
    struct TestFactory {
        applications: Mutex<Vec<Application>>,
        provider: Provider,
        connect: Mutex<Option<Arc<Gate>>>,
        catalog: Mutex<Option<Arc<Gate>>>,
        catalog_open: AtomicBool,
        connects: AtomicUsize,
        catalog_close_fails: AtomicBool,
        closing: Mutex<Option<mpsc::SyncSender<()>>>,
    }
    impl Factory for Arc<TestFactory> {
        type Provider = Provider;
        fn catalog(&self) -> Result<Vec<Application>, Error> {
            let gate = self.catalog.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.enter();
            }
            self.catalog_open.store(true, Ordering::SeqCst);
            Ok(self.applications.lock().unwrap().clone())
        }
        fn connect(&self) -> Result<Provider, Error> {
            self.connects.fetch_add(1, Ordering::SeqCst);
            let gate = self.connect.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.enter();
            }
            Ok(self.provider.clone())
        }
        fn close_catalog(&self) -> Result<(), Error> {
            self.catalog_open.store(false, Ordering::SeqCst);
            let closing = self.closing.lock().unwrap().take();
            if let Some(closing) = closing {
                closing.send(()).unwrap();
            }
            if self.catalog_close_fails.load(Ordering::SeqCst) {
                Err(Error::BackendFailed("catalog cleanup failed".into()))
            } else {
                Ok(())
            }
        }
    }
    const APP: &str = "com.example.App";
    fn fixture() -> (Arc<Server<Arc<TestFactory>>>, Arc<TestFactory>) {
        let factory = Arc::new(TestFactory {
            applications: Mutex::new(vec![Application {
                app_id: APP.into(),
                name: "App".into(),
                app: AppInstance {
                    identity: AppIdentity("private-instance-identity".into()),
                    pid: 100,
                    generation: 1,
                },
            }]),
            provider: Provider(Arc::new(Probe::default())),
            connect: Mutex::new(None),
            catalog: Mutex::new(None),
            catalog_open: AtomicBool::new(false),
            connects: AtomicUsize::new(0),
            catalog_close_fails: AtomicBool::new(false),
            closing: Mutex::new(None),
        });
        (Server::new(factory.clone()), factory)
    }
    fn begin(server: &Server<Arc<TestFactory>>, id: &str, source: &str) -> Arc<Call<Provider>> {
        match server.begin(
            json!(id),
            "exec",
            json!({"app":APP,"request_id":id,"source":source}),
        ) {
            Started::Call(call) => call,
            Started::Reply(reply) => panic!("unexpected rejection: {}", reply.data),
        }
    }
    fn reply(started: Started<Provider>) -> Reply {
        match started {
            Started::Reply(reply) => reply,
            Started::Call(_) => panic!("unexpected dispatch"),
        }
    }
    fn result(server: &Server<Arc<TestFactory>>, id: &str) -> Reply {
        server.result(json!({"app":APP,"request_id":id}))
    }

    #[test]
    fn unsettled_execution_keeps_busy_and_refreshes_the_original_action_receipt() {
        let (server, factory) = fixture();
        let (gate, control) = gate();
        *factory.provider.0.press.lock().unwrap() = Some(gate);
        let source = "const w=(await app.windows()).windows[0]; const o=await app.observe({window_ref:w.window_ref}); return await app.press({window_ref:w.window_ref,observation_ref:o.observation_ref,element_ref:o.elements[0].element_ref});";
        let call = begin(&server, "original", source);
        let app = factory.applications.lock().unwrap()[0].app.clone();
        let owner = Coordinator::new(factory.provider.clone(), app, "App".into());
        *call.owner.lock().unwrap() = Some(owner.clone());
        server.state.lock().unwrap().contexts.push_back(AppContext {
            app_id: APP.into(),
            owner: owner.clone(),
        });
        let admitted = owner.handle(Request::Submit(Submit {
            request_id: "original".into(),
            target: Target {
                app_ref: owner.app_ref.clone(),
                window_ref: None,
            },
            command: Command::Script {
                source: source.into(),
                timeout_ms: 5000,
                max_calls: 32,
            },
        }));
        assert_eq!(admitted.data["status"], "running");
        control.entered();
        owner.cancel("original");
        // Exercise the outer-wait completion boundary without spending ten seconds
        // on a production timeout. The original native action is still blocked.
        server.finish_run(&call, owner.result("original"));
        assert_eq!(result(&server, "original").data["status"], "running");
        assert_eq!(
            reply(server.begin(
                json!("next"),
                "exec",
                json!({"app":APP,"request_id":"next","source":"return 1;"})
            ))
            .data["error"]["code"],
            "busy"
        );
        control.release();
        assert!(owner.wait_idle(Duration::from_secs(5)));
        let completed = result(&server, "original");
        assert_eq!(completed.data["status"], "completed");
        let action = &completed.data["script"]["steps"][2]["receipt"];
        assert_eq!(action["outcome"]["dispatch"], "unknown");
        assert_eq!(action["provider_outcome"]["dispatch"], "dispatched");
        assert_eq!(factory.provider.0.actions.load(Ordering::SeqCst), 1);
        assert!(server.state.lock().unwrap().active.is_none());
        assert_eq!(result(&server, "original").data, completed.data);
    }

    #[test]
    fn retirement_failure_retains_the_owner_and_closes_new_dispatch() {
        let (server, factory) = fixture();
        factory
            .provider
            .0
            .shutdown_fails
            .store(true, Ordering::SeqCst);
        let app = factory.applications.lock().unwrap()[0].app.clone();
        let mut owner = Coordinator::new(factory.provider.clone(), app, "App".into());
        Arc::get_mut(&mut owner).unwrap().max_requests = 0;
        server.state.lock().unwrap().contexts.push_back(AppContext {
            app_id: APP.into(),
            owner: owner.clone(),
        });
        let call = begin(&server, "retire", "return 1;");
        let failed = server.run(&call);
        assert_eq!(failed.data["shutdown_errors"][0]["code"], "drain_failed");
        assert!(Arc::ptr_eq(
            &server.state.lock().unwrap().contexts[0].owner,
            &owner
        ));
        assert_eq!(factory.connects.load(Ordering::SeqCst), 0);
        assert_eq!(
            reply(server.begin(json!("new"), "apps", json!({}))).data["error"]["code"],
            "shutdown_failed"
        );
        assert_eq!(
            result(&server, "retire").data["shutdown_errors"][0]["message"],
            "original drain message"
        );
    }

    #[test]
    fn close_waits_for_a_preparing_owner_and_preserves_its_cleanup_failure() {
        for fail in [false, true] {
            let (server, factory) = fixture();
            factory
                .provider
                .0
                .shutdown_fails
                .store(fail, Ordering::SeqCst);
            let (gate, control) = gate();
            *factory.connect.lock().unwrap() = Some(gate);
            let (closing, noticed) = mpsc::sync_channel(0);
            *factory.closing.lock().unwrap() = Some(closing);
            let call = begin(&server, "preparing", "return 1;");
            let runner = {
                let server = server.clone();
                std::thread::spawn(move || server.run(&call))
            };
            control.entered();
            let closer = {
                let server = server.clone();
                std::thread::spawn(move || server.close())
            };
            noticed
                .recv_timeout(Duration::from_secs(5))
                .expect("close did not start");
            control.release();
            let reply = runner.join().unwrap();
            let cleanup = closer.join().unwrap();
            assert!(factory.provider.0.shutdowns.load(Ordering::SeqCst) >= 1);
            assert_eq!(cleanup.is_err(), fail);
            if fail {
                assert_eq!(reply.data["shutdown_errors"][0]["code"], "drain_failed");
                assert_eq!(
                    cleanup.unwrap_err().details.unwrap()["errors"][0]["message"],
                    "original drain message"
                );
            } else {
                assert_eq!(reply.data["error"]["code"], "cancelled");
            }
        }
    }

    #[test]
    fn catalog_cleanup_failure_on_cancel_remains_visible_in_original_result() {
        let (server, factory) = fixture();
        factory.catalog_close_fails.store(true, Ordering::SeqCst);
        let call = begin(&server, "cancelled", "return 1;");
        server.cancel(&json!("cancelled"));
        server.run(&call);
        let result = result(&server, "cancelled");
        assert_eq!(result.data["error"]["code"], "cancelled");
        assert_eq!(
            result.data["shutdown_errors"][0]["message"],
            "catalog cleanup failed"
        );
        assert_eq!(
            reply(server.begin(json!("new"), "apps", json!({}))).data["error"]["code"],
            "shutdown_failed"
        );
        assert_eq!(factory.connects.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn receipt_capacity_counts_step_inventory_evidence_separately_from_payloads() {
        let (server, _) = fixture();
        let call = begin(&server, "retained", "return 1;");
        let completed = Reply::plain(
            json!({"script":{"steps":[{"inventory":{"coverage":"supported subset","diagnostics":{"count":1}}}]},"value":"evictable result"}),
        );
        let retained = receipt_size(&completed);
        assert!(retained > completed.data["script"].to_string().len());
        server.finish_run(&call, completed);
        server.state.lock().unwrap().records[0].receipt_bytes = MAX_RECEIPT_BYTES;
        assert_eq!(
            reply(server.begin(
                json!("new"),
                "exec",
                json!({"app":APP,"request_id":"new","source":"return 2;"})
            ))
            .data["error"]["code"],
            "request_limit"
        );
        assert!(result(&server, "retained").data["script"]["steps"][0]["inventory"].is_object());
    }

    #[test]
    fn stable_app_identity_hides_instances_and_rejects_ambiguous_dispatch() {
        let (server, factory) = fixture();
        {
            let mut apps = factory.applications.lock().unwrap();
            let mut other = apps[0].clone();
            other.app.pid += 1;
            apps.push(other);
        }
        let call = match server.begin(json!("apps"), "apps", json!({})) {
            Started::Call(call) => call,
            _ => panic!("discovery rejected"),
        };
        let discovered = server.run(&call);
        assert_eq!(
            discovered.data["apps"],
            json!([{"app":APP,"name":"App","instances":2}])
        );
        assert!(!discovered
            .data
            .to_string()
            .contains("private-instance-identity"));
        let call = begin(&server, "ambiguous", "return 1;");
        assert_eq!(server.run(&call).data["error"]["code"], "ambiguous_app");
        assert_eq!(factory.connects.load(Ordering::SeqCst), 0);
        factory.applications.lock().unwrap().clear();
        assert_eq!(
            result(&server, "ambiguous").data["error"]["code"],
            "ambiguous_app"
        );
    }
    #[test]
    fn completed_owner_replaces_a_stale_running_snapshot_and_exposes_internal_cleanup_errors() {
        for fail in [false, true] {
            let (server, factory) = fixture();
            let call = begin(&server, "sampled", "return 7;");
            let app = factory.applications.lock().unwrap()[0].app.clone();
            let owner = Coordinator::new(factory.provider.clone(), app, "App".into());
            *call.owner.lock().unwrap() = Some(owner.clone());
            server.state.lock().unwrap().contexts.push_back(AppContext {
                app_id: APP.into(),
                owner: owner.clone(),
            });
            let admitted = owner.handle(Request::Submit(Submit {
                request_id: "sampled".into(),
                target: Target {
                    app_ref: owner.app_ref.clone(),
                    window_ref: None,
                },
                command: Command::Script {
                    source: "return 7;".into(),
                    timeout_ms: 5000,
                    max_calls: 32,
                },
            }));
            assert_eq!(admitted.data["status"], "running");
            assert!(owner.wait_idle(Duration::from_secs(5)));
            if fail {
                factory
                    .provider
                    .0
                    .shutdown_fails
                    .store(true, Ordering::SeqCst);
                assert!(owner.close().is_error);
            }
            let completed = server.finish_run(&call, admitted);
            assert_eq!(completed.data["status"], "completed");
            assert_eq!(completed.data["value"]["result"], 7);
            assert_eq!(result(&server, "sampled").data["status"], "completed");
            if fail {
                assert_eq!(completed.data["shutdown_errors"][0]["code"], "drain_failed");
                assert_eq!(
                    reply(server.begin(json!("new"), "apps", json!({}))).data["error"]["code"],
                    "shutdown_failed"
                );
            }
        }
    }

    #[test]
    fn cancellation_reaps_a_catalog_reader_created_after_the_initial_stop() {
        let (server, factory) = fixture();
        let (gate, control) = gate();
        *factory.catalog.lock().unwrap() = Some(gate);
        let call = begin(&server, "late-catalog", "return 1;");
        let runner = {
            let server = server.clone();
            std::thread::spawn(move || server.run(&call))
        };
        control.entered();
        server.cancel(&json!("late-catalog"));
        control.release();
        assert_eq!(runner.join().unwrap().data["error"]["code"], "cancelled");
        assert!(!factory.catalog_open.load(Ordering::SeqCst));
        assert_eq!(factory.connects.load(Ordering::SeqCst), 0);
    }
}

/// Discovery presents logical app IDs, while execution resolves instances internally.
pub(super) fn public_apps(apps: Vec<Application>) -> Vec<Value> {
    let mut grouped: Vec<(String, String, usize)> = Vec::new();
    for entry in apps {
        if let Some((_, _, count)) = grouped.iter_mut().find(|(id, _, _)| *id == entry.app_id) {
            *count += 1;
        } else {
            grouped.push((entry.app_id, entry.name, 1));
        }
    }
    grouped
        .into_iter()
        .map(|(id, name, count)| {
            let mut row = json!({"app":id,"name":name});
            if count > 1 {
                row["instances"] = json!(count);
            }
            row
        })
        .collect()
}

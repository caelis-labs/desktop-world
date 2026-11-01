//! Standard-tool orchestration. Selection is explicit; approval belongs to the MCP client.
use super::{
    app_id, problem, scripts,
    wire::{self, Command, Request, Submit, Target, WireError},
    Coordinator, Reply,
};
use crate::computer::{AppInstance, Backend, Error, InputPolicy};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

// Completed receipts use a bounded sliding window. Active/unsettled execution
// and the newest result remain available; history never expires the app context.
const RETENTION: super::RetentionPolicy = super::RetentionPolicy::DEFAULT;
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
    fn permissions(&self) -> Option<Value> {
        None
    }
    fn input_policy(&self) -> InputPolicy {
        InputPolicy::Auto
    }
    fn description(&self) -> Value {
        json!({"coverage":"Provider-defined application window subset","conditions":[]})
    }
    /// Stops a possibly blocked metadata read. A later call may create a new reader.
    fn close_catalog(&self) -> Result<(), Error>;
    /// Terminal session resources, independent of refreshable catalog/app owners.
    fn close_session(&self) -> Result<(), Error> {
        Ok(())
    }
}
struct Record {
    id: String,
    rpc_id: Value,
    app: String,
    input: Value,
    reply: Option<Reply>,
    payload_bytes: usize,
    receipt_bytes: usize,
}
struct AppContext<B: Backend> {
    app_id: String,
    instances: Vec<AppInstance>,
    // One surviving native instance preserves logical app data. Replacing
    // every member resets it; individual joins/exits only retire their refs.
    script_state: Arc<Mutex<Value>>,
    owner: Option<Arc<Coordinator<B>>>,
}
struct State<B: Backend> {
    // One logical entry per used app, independent of execution history retention.
    // Includes preparing and failed-to-retire owners until cleanup succeeds.
    contexts: VecDeque<AppContext<B>>,
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
    execution_id: Option<String>,
    command: ToolCall,
    run_finished: AtomicBool,
    completion: tokio::sync::Notify,
    // Duplicate waiters own this response even if later calls evict its history.
    completed: Mutex<Option<Reply>>,
    // Set and cleared under Server state; pins admission while shared cleanup runs.
    stopping: AtomicBool,
    context_reset: AtomicBool,
    state_reset: AtomicBool,
}
#[derive(Clone)]
enum ToolCall {
    Apps,
    Exec {
        app: String,
        source: String,
        timeout_ms: u64,
        max_calls: u32,
    },
}
pub enum Started<B: Backend> {
    Reply(Reply),
    Call(Arc<Call<B>>),
    // The original accepted call will produce the response for this RPC ID.
    InFlight(Arc<Call<B>>),
}

impl<B: Backend> Call<B> {
    pub(super) async fn wait_finished(&self) {
        loop {
            // notify_waiters observes futures created before notification,
            // even before they are polled. Check the sole state after creating it.
            let finished = self.completion.notified();
            if self.run_finished.load(Ordering::Acquire) {
                return;
            }
            finished.await;
        }
    }
}

impl<F: Factory> Server<F> {
    pub fn new(factory: F) -> Arc<Self> {
        Arc::new(Self {
            factory,
            state: Mutex::new(State {
                contexts: VecDeque::new(),
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
        let mut state = self.state.lock().unwrap();
        self.refresh(&mut state);
        // Standard MCP request identity is transport-owned. Repeated frames
        // refer to one accepted execution while its history is retained, including
        // while it is still running. Old IDs are not kept in an unbounded tombstone set.
        // A new RPC ID always admits new work, even for the same source text.
        if let Some(record) = state.records.iter().find(|record| record.rpc_id == rpc_id) {
            if name != "exec" || record.input != args {
                return Started::Reply(problem(
                    "request_conflict",
                    "This MCP request ID already belongs to different input in this session",
                ));
            }
            if let Some(call) = state
                .active
                .as_ref()
                .filter(|call| call.rpc_id == rpc_id && !call.run_finished.load(Ordering::Acquire))
            {
                return Started::InFlight(call.clone());
            }
            return Started::Reply(self.record_reply(&state, record));
        }
        if name == "result" {
            drop(state);
            return Started::Reply(self.result(args));
        }
        if name == "api" {
            if state.closed {
                return Started::Reply(problem("closed", "The MCP server is shutting down"));
            }
            return Started::Reply(match super::description::requested(&args) {
                Ok(mut value) => {
                    value["service"] = self.service();
                    Reply::plain(value)
                }
                Err(error) => Reply::error(error),
            });
        }
        let command = match parse_call(name, &args) {
            Ok(c) => c,
            Err(e) => return Started::Reply(Reply::error(e)),
        };
        if state.active.as_ref().is_some_and(|c| c.rpc_id == rpc_id) {
            return Started::Reply(problem(
                "duplicate_rpc_id",
                "The MCP request ID is already active",
            ));
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
        let execution_id = match &command {
            ToolCall::Apps => None,
            ToolCall::Exec { app, .. } => {
                let id = match super::reference("exe") {
                    Ok(id) => id,
                    Err(error) => return Started::Reply(Reply::error(wire::core_error(&error))),
                };
                state.records.push_back(Record {
                    id: id.clone(),
                    rpc_id: rpc_id.clone(),
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
            execution_id,
            command,
            run_finished: AtomicBool::new(false),
            completion: tokio::sync::Notify::new(),
            completed: Mutex::new(None),
            stopping: AtomicBool::new(false),
            context_reset: AtomicBool::new(false),
            state_reset: AtomicBool::new(false),
        });
        state.active = Some(call.clone());
        trim_history(&mut state);
        Started::Call(call)
    }
    pub fn run(&self, call: &Arc<Call<F::Provider>>) -> Reply {
        let reply = match &call.command {
            ToolCall::Apps => self.apps(call),
            ToolCall::Exec {
                app,
                source,
                timeout_ms,
                max_calls,
            } => self.execute(
                call,
                app,
                call.execution_id
                    .as_deref()
                    .expect("accepted execution has an ID"),
                source,
                *timeout_ms,
                *max_calls,
            ),
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
            if let Some((owner, id)) = owner.as_ref().zip(call.execution_id.as_ref()) {
                let current = owner.result(id);
                if current.data["error"]["code"] != "unknown_request" {
                    // The job may have completed after execute took a running
                    // snapshot. Never freeze that stale snapshot as terminal.
                    reply = current;
                }
            }
        }
        if call.execution_id.is_some() && !unsettled {
            // Accepted requests rejected before Core submission still have a
            // terminal record (for example cancellation during discovery).
            reply.data["status"] = json!("completed");
        }
        self.annotate(&state, call, &mut reply);
        // A timeout of the outer wait is not a terminal native receipt. Keep the
        // owner and global reservation until its actual job has finished.
        if !unsettled {
            if let Some(id) = &call.execution_id {
                cache_reply(&mut state, id, &reply);
            }
            if !call.stopping.load(Ordering::Acquire) {
                if let Some((owner, id)) = owner.as_ref().zip(call.execution_id.as_ref()) {
                    owner.release_completed_job(id);
                }
                if state
                    .active
                    .as_ref()
                    .is_some_and(|active| Arc::ptr_eq(active, call))
                {
                    state.active = None;
                }
            }
        }
        *call.completed.lock().unwrap() = Some(reply.clone());
        call.run_finished.store(true, Ordering::Release);
        call.completion.notify_waiters();
        self.finished.notify_all();
        // Preserve the original first-delivery payload independently of history
        // eviction. Only its public completion state waits for the reservation.
        if call.execution_id.is_some()
            && state
                .active
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, call))
        {
            reply.data["status"] = json!("running");
        }
        reply
    }
    fn refresh(&self, state: &mut State<F::Provider>) {
        let Some(call) = state.active.clone().filter(|call| {
            call.run_finished.load(Ordering::Acquire) && !call.stopping.load(Ordering::Acquire)
        }) else {
            return;
        };
        let owner = call.owner.lock().unwrap().clone();
        if let Some(owner) = owner {
            if inspect_owner(state, &owner) {
                return;
            }
            if let Some(id) = &call.execution_id {
                let mut reply = owner.result(id);
                self.annotate(state, &call, &mut reply);
                cache_reply(state, id, &reply);
                *call.completed.lock().unwrap() = Some(reply);
                owner.release_completed_job(id);
            }
        }
        state.active = None;
    }
    fn annotate(&self, state: &State<F::Provider>, call: &Call<F::Provider>, reply: &mut Reply) {
        if let ToolCall::Exec { app, .. } = &call.command {
            public_execution(reply, app, call.execution_id.as_deref().unwrap());
            reply.data["context_reset"] = json!(call.context_reset.load(Ordering::Acquire));
            reply.data["state_reset"] = json!(call.state_reset.load(Ordering::Acquire));
        }
        append_shutdown_errors(reply, &state.shutdown_errors);
    }
    fn service(&self) -> Value {
        json!({"input_policy":self.factory.input_policy().as_str()})
    }
    fn apps(&self, call: &Call<F::Provider>) -> Reply {
        if call.cancelled.load(Ordering::Acquire) {
            return problem("cancelled", "Application discovery was cancelled");
        }
        match self.factory.catalog() {
            Ok(apps) if !call.cancelled.load(Ordering::Acquire) => {
                let apps = public_apps(apps);
                Reply::plain(json!({"version":1,
                "apps":apps,"permissions":self.factory.permissions(),"backend":self.factory.description(),"service":self.service(),
                "script":super::description::index(),
                "limits":{"retained_executions":RETENTION.records,"payload_bytes":RETENTION.payload_bytes,
                    "receipt_bytes":RETENTION.receipt_bytes,"max_steps_per_request":scripts::MAX_CALLS},
                "semantics":"Every exec requires a stable application identifier. Windows from all its running instances are aggregated; window_ref routes each action to its exact native owner. Membership refreshes before each exec. Switching apps preserves logical state and native references. Instance joins/exits preserve surviving references and committed state; replacing every instance clears state and sets state_reset. Native owner replacement invalidates references and sets context_reset. This connection executes one call at a time. DTW assigns execution_id automatically. result(app) reads the latest retained execution for that app; optional execution_id selects an older retained record. History and MCP request deduplication use a bounded sliding window; expired results are unavailable, not evidence of no effect. Identical requests reuse an execution only while its MCP request ID is retained. New or expired MCP request IDs execute new work; do not resubmit uncertain actions. App state and native references outlive history eviction."}))
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
        let entries = match self.factory.catalog() {
            Ok(apps) => apps
                .into_iter()
                .filter(|entry| entry.app_id == app_id)
                .collect::<Vec<_>>(),
            Err(e) => return Reply::error(wire::core_error(&e)),
        };
        let Some(first) = entries.first() else {
            return problem(
                "app_unavailable",
                "The application is not running. Discover again; no script was dispatched",
            );
        };
        let app_name = first.name.clone();
        let mut instances = Vec::new();
        for entry in entries {
            if !instances.contains(&entry.app) {
                instances.push(entry.app);
            }
        }
        if let Err(error) = crate::runtime::validate_app_instances(&instances) {
            return Reply::error(wire::core_error(&error));
        }
        if call.cancelled.load(Ordering::Acquire) {
            return problem("cancelled", "Cancelled before script admission");
        }
        let (candidate, continuous) = {
            let state = self.state.lock().unwrap();
            match state
                .contexts
                .iter()
                .find(|context| context.app_id == app_id)
            {
                Some(context) => (
                    context.owner.clone(),
                    context.instances.iter().any(|app| instances.contains(app)),
                ),
                None => (None, false),
            }
        };
        let existing = match candidate {
            Some(owner) => {
                let reusable = {
                    let state = owner.state.lock().unwrap();
                    continuous && state.terminal.is_none() && owner.runtime.provider_available()
                };
                if reusable {
                    // Reconciliation never closes the shared native provider or
                    // changes committed state when another member is still live.
                    if let Err(error) = owner.reconcile_instances(instances.clone()) {
                        let cleanup = owner.state.lock().unwrap().shutdown_error.clone();
                        if let Some(cleanup) = cleanup {
                            self.remember_shutdown(cleanup);
                        }
                        return Reply::error(wire::core_error(&error));
                    }
                    self.state
                        .lock()
                        .unwrap()
                        .contexts
                        .iter_mut()
                        .find(|context| context.app_id == app_id)
                        .unwrap()
                        .instances = instances.clone();
                    Some(owner)
                } else {
                    if let Err(reply) = self.retire(&owner) {
                        return reply;
                    }
                    call.context_reset.store(true, Ordering::Release);
                    None
                }
            }
            None => None,
        };
        let owner = if let Some(owner) = existing {
            owner
        } else {
            let script_state = {
                let mut state = self.state.lock().unwrap();
                if !state.shutdown_errors.is_empty() {
                    return shutdown_reply(&state.shutdown_errors);
                }
                if state.closed || call.cancelled.load(Ordering::Acquire) {
                    return problem("cancelled", "Stopped before creating an app context");
                }
                if let Some(context) = state.contexts.iter_mut().find(|c| c.app_id == app_id) {
                    call.context_reset.store(true, Ordering::Release);
                    if !continuous {
                        context.script_state = Arc::new(Mutex::new(json!({})));
                        call.state_reset.store(true, Ordering::Release);
                    }
                    context.instances = instances.clone();
                    context.script_state.clone()
                } else {
                    let script_state = Arc::new(Mutex::new(json!({})));
                    state.contexts.push_back(AppContext {
                        app_id: app_id.into(),
                        instances: instances.clone(),
                        script_state: script_state.clone(),
                        owner: None,
                    });
                    script_state
                }
            };
            let backend = match self.factory.connect() {
                Ok(backend) => backend,
                Err(error) => return Reply::error(wire::core_error(&error)),
            };
            let owner =
                match Coordinator::with_app_instances(backend, instances, app_name, script_state) {
                    Ok(owner) => owner,
                    Err(error) => return Reply::error(wire::core_error(&error)),
                };
            // Register even if close/cancel won during connect. close() waits for
            // this run and performs a second pass over all registered owners.
            self.state
                .lock()
                .unwrap()
                .contexts
                .iter_mut()
                .find(|context| context.app_id == app_id)
                .expect("logical app context was registered before connecting")
                .owner = Some(owner.clone());
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
        if let Some(context) = state.contexts.iter_mut().find(|entry| {
            entry
                .owner
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, owner))
        }) {
            context.owner = None;
        }
        Ok(())
    }
    fn remember_shutdown(&self, error: WireError) {
        remember_shutdown(&mut self.state.lock().unwrap(), error);
    }
    pub(super) fn completed_reply(&self, call: &Call<F::Provider>) -> Reply {
        let mut state = self.state.lock().unwrap();
        self.refresh(&mut state);
        match call
            .execution_id
            .as_ref()
            .and_then(|id| state.records.iter().find(|record| record.id == *id))
        {
            Some(record) => self.record_reply(&state, record),
            None => {
                let mut reply = call.completed.lock().unwrap().clone().unwrap_or_else(|| {
                    problem("execution_unavailable", "The original execution is not retained; no work was replayed. Missing history does not prove no effect")
                });
                append_shutdown_errors(&mut reply, &state.shutdown_errors);
                reply
            }
        }
    }
    fn record_reply(&self, state: &State<F::Provider>, record: &Record) -> Reply {
        let active = state
            .active
            .as_ref()
            .filter(|active| active.execution_id.as_deref() == Some(&record.id));
        let mut reply = if let Some(reply) = &record.reply {
            reply.clone()
        } else {
            active
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
        if let Some(active) = active {
            // Core can finish before the Server worker publishes its cache.
            // Project the same app metadata on both sides of that boundary.
            self.annotate(state, active, &mut reply);
            // A terminal Core receipt does not retire shared catalog cleanup
            // or the outer worker. refresh() releases this reservation only
            // when both are settled, so completed also means later admission
            // is no longer blocked by this execution.
            reply.data["status"] = json!("running");
        } else {
            public_execution(&mut reply, &record.app, &record.id);
            append_shutdown_errors(&mut reply, &state.shutdown_errors);
        }
        reply
    }
    fn result(&self, args: Value) -> Reply {
        let parsed = (|| {
            exact_object(&args, &["app", "execution_id"])?;
            let id = args.get("execution_id").map(execution_id).transpose()?;
            Ok::<_, WireError>((app_id::parse(&args["app"])?, id))
        })();
        let (app, id) = match parsed {
            Ok(value) => value,
            Err(error) => return Reply::error(error),
        };
        let mut state = self.state.lock().unwrap();
        self.refresh(&mut state);
        if let Some(id) = id {
            match state.records.iter().find(|record| record.id == id) {
                Some(record) if record.app == app => self.record_reply(&state, record),
                Some(_) => problem("wrong_app", "The execution belongs to a different app identifier"),
                None => problem("execution_unavailable", "This execution is not retained in this MCP session; it may have expired or never existed here. Missing history does not prove no effect; no work was replayed"),
            }
        } else {
            match state.records.iter().rev().find(|record| record.app == app) {
                Some(record) => {
                    let mut reply = self.record_reply(&state, record);
                    reply.data["selection"] = json!("latest_accepted");
                    reply
                }
                None => problem("execution_unavailable", "No execution is retained for this app; older results may have expired. Missing history does not prove no effect; no work was replayed"),
            }
        }
    }
    pub fn cancel(&self, rpc_id: &Value) {
        let active = self.state.lock().unwrap().active.clone();
        if let Some(call) = active.filter(|call| call.rpc_id == *rpc_id) {
            self.cancel_call(&call);
        }
    }
    /// A dropped SDK handler may finish cleanup late; only stop its own call.
    pub(super) fn cancel_call(&self, call: &Arc<Call<F::Provider>>) {
        {
            let state = self.state.lock().unwrap();
            if !state
                .active
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, call))
                || call.stopping.load(Ordering::Acquire)
            {
                return;
            }
            // finish_run and refresh keep this reservation until the shared
            // catalog cleanup ends. No native I/O runs under the Server lock.
            call.stopping.store(true, Ordering::Release);
            call.cancelled.store(true, Ordering::Release);
        }
        let cancelled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let owner = call.owner.lock().unwrap().clone();
            owner
                .as_ref()
                .zip(call.execution_id.as_ref())
                .map(|(owner, id)| owner.cancel(id))
        }));
        // Try both cleanups even if one panics; retain failure before releasing
        // the reservation so a panic cannot silently reopen admission.
        let catalog = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.factory.close_catalog()
        }));
        let mut state = self.state.lock().unwrap();
        match cancelled {
            Ok(reply) => {
                if let Some(error) = reply.as_ref().and_then(cleanup_error) {
                    remember_shutdown(&mut state, error);
                }
            }
            Err(_) => remember_shutdown(
                &mut state,
                shutdown_error("Application cancellation panicked; native outcome remains unknown"),
            ),
        }
        match catalog {
            Ok(Ok(())) => {}
            Ok(Err(error)) => remember_shutdown(&mut state, wire::core_error(&error)),
            Err(_) => remember_shutdown(
                &mut state,
                shutdown_error("Catalog cancellation panicked; native outcome remains unknown"),
            ),
        }
        call.stopping.store(false, Ordering::Release);
        self.finished.notify_all();
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
        if let Err(error) = self.factory.close_session() {
            self.remember_shutdown(wire::core_error(&error));
        }
        if let Some(call) = active {
            let until = Instant::now() + STOP_TIMEOUT;
            let mut state = self.state.lock().unwrap();
            while !call.run_finished.load(Ordering::Acquire)
                || call.stopping.load(Ordering::Acquire)
            {
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                state = self.finished.wait_timeout(state, left).unwrap().0;
            }
            let unfinished =
                !call.run_finished.load(Ordering::Acquire) || call.stopping.load(Ordering::Acquire);
            drop(state);
            if unfinished {
                self.remember_shutdown(shutdown_error(
                    "Preparing, running or cancellation cleanup did not settle",
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
            .filter_map(|context| context.owner.clone())
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
// Internal Coordinator jobs still use request_id; the public tool boundary owns
// execution identifiers and never asks the model to manufacture one.
fn public_execution(reply: &mut Reply, app: &str, id: &str) {
    reply.data.as_object_mut().unwrap().remove("request_id");
    reply.data["app"] = json!(app);
    reply.data["execution_id"] = json!(id);
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
// The active call may have a terminal-looking snapshot before native cleanup
// settles. Keep it, and keep the newest result for delivery, regardless of size.
// All older completed history is disposable, including unknown outcomes: losing
// a receipt is not authority to repeat its side effects.
fn trim_history<B: Backend>(state: &mut State<B>) {
    let active = state
        .active
        .as_ref()
        .and_then(|call| call.execution_id.as_deref());
    let latest = state.records.back().map(|record| record.id.clone());
    let mut bytes: usize = state
        .records
        .iter()
        .map(|record| record.receipt_bytes)
        .sum();
    while state.records.len() > RETENTION.records || bytes > RETENTION.receipt_bytes {
        let removable = state.records.iter().position(|record| {
            record.reply.is_some()
                && Some(record.id.as_str()) != active
                && Some(&record.id) != latest.as_ref()
        });
        let Some(index) = removable else { break };
        let removed = state.records.remove(index).unwrap();
        bytes = bytes.saturating_sub(removed.receipt_bytes);
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
        if bytes <= RETENTION.payload_bytes {
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
    trim_history(state);
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
fn execution_id(value: &Value) -> Result<String, WireError> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= 128)
        .map(str::to_owned)
        .ok_or_else(|| invalid("execution_id must contain 1 to 128 characters"))
}
fn parse_call(name: &str, args: &Value) -> Result<ToolCall, WireError> {
    match name {
        "apps" => {
            exact_object(args, &[])?;
            Ok(ToolCall::Apps)
        }
        "exec" => {
            exact_object(args, &["app", "source"])?;
            let app = app_id::parse(&args["app"])?;
            let source = args["source"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= scripts::MAX_SOURCE_BYTES)
                .ok_or_else(|| {
                    invalid("source must be a nonempty JavaScript function body of at most 64 KiB")
                })?
                .to_owned();
            let (timeout_ms, max_calls) = scripts::budgets(args.as_object().unwrap())?;
            Ok(ToolCall::Exec {
                app,
                source,
                timeout_ms,
                max_calls,
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
        action_targets: Mutex<Vec<WindowTarget>>,
        stopped: AtomicBool,
        timeout_read: AtomicBool,
        shutdown_fails: AtomicBool,
        cleanup_fails: AtomicBool,
    }
    #[derive(Clone)]
    struct Provider(Arc<Probe>);
    impl Backend for Provider {
        type Binding = ();
        fn is_available(&self) -> bool {
            !self.0.stopped.load(Ordering::SeqCst)
        }
        fn stamp(&self) -> ProviderStamp {
            ProviderStamp {
                id: "server-test".into(),
                generation: 1,
            }
        }
        fn supports(&self, capability: Capability) -> bool {
            matches!(
                capability,
                Capability::Windows | Capability::Observe | Capability::Press
            )
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
        fn cleanup_error(&self) -> Option<Error> {
            self.0
                .cleanup_fails
                .load(Ordering::SeqCst)
                .then(|| Error::NativeFailure {
                    code: "binding_release_failed".into(),
                    message: "Original binding cleanup failure".into(),
                })
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
            if self.0.timeout_read.swap(false, Ordering::SeqCst) {
                self.0.stopped.store(true, Ordering::SeqCst);
                return Err(Error::TimedOut);
            }
            if self.0.stopped.load(Ordering::SeqCst) {
                return Err(Error::BackendFailed("native transport: Stopped".into()));
            }
            Ok(BackendObservation {
                discovery: None,
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
            target: &WindowTarget,
            _: &(),
            _: ElementId,
            _: &CallContext<'_>,
        ) -> ActionOutcome {
            let gate = self.0.press.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.enter();
            }
            self.0.actions.fetch_add(1, Ordering::SeqCst);
            self.0.action_targets.lock().unwrap().push(target.clone());
            ActionOutcome {
                delivery: None,
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
        connect_fails: AtomicBool,
        catalog_close_fails: AtomicBool,
        catalog_close_panics: AtomicBool,
        catalog_close: Mutex<Option<Arc<Gate>>>,
        catalog_closes: AtomicUsize,
        session_closes: AtomicUsize,
        closing: Mutex<Option<mpsc::SyncSender<()>>>,
    }
    impl Factory for Arc<TestFactory> {
        type Provider = Provider;
        fn close_session(&self) -> Result<(), Error> {
            self.session_closes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
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
            if self.connect_fails.load(Ordering::SeqCst) {
                return Err(Error::BackendFailed("connect failed".into()));
            }
            self.provider.0.stopped.store(false, Ordering::SeqCst);
            Ok(self.provider.clone())
        }
        fn close_catalog(&self) -> Result<(), Error> {
            self.catalog_closes.fetch_add(1, Ordering::SeqCst);
            let gate = self.catalog_close.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.enter();
            }
            assert!(
                !self.catalog_close_panics.swap(false, Ordering::SeqCst),
                "controlled catalog close panic"
            );
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
            connect_fails: AtomicBool::new(false),
            catalog_close_fails: AtomicBool::new(false),
            catalog_close_panics: AtomicBool::new(false),
            catalog_close: Mutex::new(None),
            catalog_closes: AtomicUsize::new(0),
            session_closes: AtomicUsize::new(0),
            closing: Mutex::new(None),
        });
        (Server::new(factory.clone()), factory)
    }
    fn begin(server: &Server<Arc<TestFactory>>, id: &str, source: &str) -> Arc<Call<Provider>> {
        match server.begin(json!(id), "exec", json!({"app":APP,"source":source})) {
            Started::Call(call) => call,
            Started::Reply(reply) => panic!("unexpected rejection: {}", reply.data),
            Started::InFlight(_) => panic!("unexpected duplicate frame"),
        }
    }
    #[test]
    fn cancelled_app_work_keeps_shared_session_resources_until_terminal_close() {
        let (server, factory) = fixture();
        let call = begin(&server, "cancelled-before-start", "return 1");
        server.cancel(&json!("cancelled-before-start"));
        server.run(&call);
        assert!(factory.catalog_closes.load(Ordering::SeqCst) > 0);
        assert_eq!(factory.session_closes.load(Ordering::SeqCst), 0);
        server.close().unwrap();
        assert_eq!(factory.session_closes.load(Ordering::SeqCst), 1);
    }
    fn reply(started: Started<Provider>) -> Reply {
        match started {
            Started::Reply(reply) => reply,
            Started::Call(_) => panic!("unexpected dispatch"),
            Started::InFlight(_) => panic!("unexpected pending response"),
        }
    }
    fn result(server: &Server<Arc<TestFactory>>, id: &str) -> Reply {
        let execution_id = server
            .state
            .lock()
            .unwrap()
            .records
            .iter()
            .find(|record| record.rpc_id == json!(id))
            .unwrap()
            .id
            .clone();
        server.result(json!({"app":APP,"execution_id":execution_id}))
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
            instances: owner.instances(),
            script_state: owner.state.lock().unwrap().script_state.clone(),
            owner: Some(owner.clone()),
        });
        let admitted = owner.handle(Request::Submit(Submit {
            request_id: call.execution_id.clone().unwrap(),
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
        owner.release_completed_job(call.execution_id.as_deref().unwrap());
        assert_eq!(owner.state.lock().unwrap().jobs.len(), 1);
        owner.cancel(call.execution_id.as_deref().unwrap());
        // Exercise the outer-wait completion boundary without spending ten seconds
        // on a production timeout. The original native action is still blocked.
        server.finish_run(&call, owner.result(call.execution_id.as_deref().unwrap()));
        assert_eq!(result(&server, "original").data["status"], "running");
        assert_eq!(
            reply(server.begin(
                json!("next"),
                "exec",
                json!({"app":APP,"source":"return 1;"})
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
        let owner = Coordinator::new(factory.provider.clone(), app, "App".into());
        owner.state.lock().unwrap().terminal = Some(Error::Closed);
        server.state.lock().unwrap().contexts.push_back(AppContext {
            app_id: APP.into(),
            instances: owner.instances(),
            script_state: owner.state.lock().unwrap().script_state.clone(),
            owner: Some(owner.clone()),
        });
        let call = begin(&server, "retire", "return 1;");
        let failed = server.run(&call);
        assert_eq!(failed.data["shutdown_errors"][0]["code"], "drain_failed");
        assert!(Arc::ptr_eq(
            server.state.lock().unwrap().contexts[0]
                .owner
                .as_ref()
                .unwrap(),
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
    fn failed_native_reconnect_retains_the_logical_state_and_original_failure() {
        let (server, factory) = fixture();
        let app = factory.applications.lock().unwrap()[0].app.clone();
        let owner = Coordinator::new(factory.provider.clone(), app, "App".into());
        owner.state.lock().unwrap().terminal = Some(Error::Closed);
        let script_state = Arc::new(Mutex::new(json!({"kept":7})));
        owner.state.lock().unwrap().script_state = script_state.clone();
        server.state.lock().unwrap().contexts.push_back(AppContext {
            app_id: APP.into(),
            instances: owner.instances(),
            script_state,
            owner: Some(owner.clone()),
        });
        factory.connect_fails.store(true, Ordering::SeqCst);
        let failed = server.run(&begin(&server, "failed-connect", "return state.kept;"));
        assert!(failed.is_error);
        assert_eq!(failed.data["context_reset"], true);
        assert_eq!(failed.data["state_reset"], false);
        {
            let state = server.state.lock().unwrap();
            assert!(state.contexts[0].owner.is_none());
            assert_eq!(
                *state.contexts[0].script_state.lock().unwrap(),
                json!({"kept":7})
            );
        }
        factory.connect_fails.store(false, Ordering::SeqCst);
        let recovered = server.run(&begin(&server, "connected", "return state.kept;"));
        assert!(!recovered.is_error, "{}", recovered.data);
        assert_eq!(recovered.data["value"]["result"], 7);
        assert_eq!(recovered.data["context_reset"], true);
        assert_eq!(recovered.data["state_reset"], false);
        assert_eq!(result(&server, "failed-connect").data, failed.data);
        assert_eq!(factory.connects.load(Ordering::SeqCst), 2);
        server.close().unwrap();
    }

    #[test]
    fn stopped_provider_recovers_only_on_a_new_execution_with_fresh_references() {
        let (server, factory) = fixture();
        let original = server.run(&begin(
            &server,
            "commit",
            r#"
            state.window=(await app.windows()).windows[0].window_ref;
            const observed=await app.observe({window_ref:state.window});
            await app.press({window_ref:state.window,observation_ref:observed.observation_ref,
                element_ref:observed.elements[0].element_ref});
            state.kept=7; return state.window;
        "#,
        ));
        assert!(!original.is_error, "{}", original.data);
        let old_window = original.data["value"]["result"]
            .as_str()
            .unwrap()
            .to_owned();
        factory
            .provider
            .0
            .timeout_read
            .store(true, Ordering::SeqCst);
        let failed = server.run(&begin(
            &server,
            "timed-read",
            "return await app.observe({window_ref:state.window});",
        ));
        assert!(failed.is_error, "{}", failed.data);
        assert_eq!(
            failed.data["script"]["steps"][0]["error"]["code"],
            "timed_out"
        );
        assert!(failed.data["script"]["steps"][0]["receipt"].is_null());
        assert_eq!(factory.connects.load(Ordering::SeqCst), 1);
        assert_eq!(factory.provider.0.actions.load(Ordering::SeqCst), 1);

        // Logical state survives, but strings embedded in it remain old refs.
        // Native references are never silently rebound to the new provider.
        let recovered = server.run(&begin(
            &server,
            "fresh-inventory",
            r#"
            return {kept:state.kept,old:state.window,windows:(await app.windows()).windows};
        "#,
        ));
        assert!(!recovered.is_error, "{}", recovered.data);
        assert_eq!(recovered.data["context_reset"], true);
        assert_eq!(recovered.data["state_reset"], false);
        assert_eq!(recovered.data["value"]["result"]["kept"], 7);
        assert_eq!(recovered.data["value"]["result"]["old"], old_window);
        assert_ne!(
            recovered.data["value"]["result"]["windows"][0]["window_ref"],
            old_window
        );
        assert_eq!(factory.connects.load(Ordering::SeqCst), 2);
        let stale = server.run(&begin(
            &server,
            "old-reference",
            "return await app.observe({window_ref:state.window});",
        ));
        assert!(stale.is_error, "{}", stale.data);
        assert_eq!(
            stale.data["script"]["steps"][0]["error"]["code"],
            "target_changed"
        );
        assert_eq!(result(&server, "timed-read").data, failed.data);
        assert_eq!(result(&server, "commit").data, original.data);
        assert_eq!(factory.provider.0.actions.load(Ordering::SeqCst), 1);
        server.close().unwrap();
    }

    #[test]
    fn stopped_provider_is_not_replaced_when_its_cleanup_failed() {
        let (server, factory) = fixture();
        let original = server.run(&begin(&server, "original", "return 7;"));
        assert!(!original.is_error);
        factory.provider.0.stopped.store(true, Ordering::SeqCst);
        factory
            .provider
            .0
            .shutdown_fails
            .store(true, Ordering::SeqCst);
        let failed = server.run(&begin(&server, "fresh", "return await app.windows();"));
        assert_eq!(failed.data["shutdown_errors"][0]["code"], "drain_failed");
        assert_eq!(factory.connects.load(Ordering::SeqCst), 1);
        assert_eq!(factory.provider.0.actions.load(Ordering::SeqCst), 0);
        assert!(server.state.lock().unwrap().contexts[0].owner.is_some());
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
    fn cancellation_cleanup_keeps_admission_until_shared_reader_is_retired() {
        let (server, factory) = fixture();
        let call = begin(&server, "cancel-owner", "return 7;");
        let (gate, control) = gate();
        *factory.catalog_close.lock().unwrap() = Some(gate);
        let canceller = {
            let server = server.clone();
            let call = call.clone();
            std::thread::spawn(move || server.cancel_call(&call))
        };
        control.entered();
        let completed = server.run(&call);
        assert_eq!(completed.data["error"]["code"], "cancelled");
        assert_eq!(completed.data["status"], "running");
        assert!(call.run_finished.load(Ordering::Acquire));
        assert_eq!(
            reply(server.begin(json!("during-cleanup"), "apps", json!({}))).data["error"]["code"],
            "busy"
        );
        let closes = factory.catalog_closes.load(Ordering::SeqCst);
        server.cancel_call(&call);
        assert_eq!(factory.catalog_closes.load(Ordering::SeqCst), closes);
        assert!(call.stopping.load(Ordering::Acquire));
        control.release();
        canceller.join().unwrap();
        let next = match server.begin(json!("after-cleanup"), "apps", json!({})) {
            Started::Call(call) => call,
            _ => panic!("cleanup did not release admission"),
        };
        assert!(!server.run(&next).is_error);
        assert!(factory.catalog_open.load(Ordering::SeqCst));
        let closes = factory.catalog_closes.load(Ordering::SeqCst);
        server.cancel_call(&call);
        assert!(factory.catalog_open.load(Ordering::SeqCst));
        assert_eq!(factory.catalog_closes.load(Ordering::SeqCst), closes);
        let mut expected = completed.data;
        expected["status"] = json!("completed");
        assert_eq!(result(&server, "cancel-owner").data, expected);
        server.close().unwrap();
    }

    #[test]
    fn close_waits_for_cancellation_cleanup_after_execution_has_finished() {
        let (server, factory) = fixture();
        let call = begin(&server, "cancel-owner", "return 7;");
        let (gate, control) = gate();
        *factory.catalog_close.lock().unwrap() = Some(gate);
        let canceller = {
            let server = server.clone();
            let call = call.clone();
            std::thread::spawn(move || server.cancel_call(&call))
        };
        control.entered();
        server.run(&call);
        assert!(call.run_finished.load(Ordering::Acquire));
        let (closing, noticed) = mpsc::sync_channel(0);
        *factory.closing.lock().unwrap() = Some(closing);
        let (closed, finished) = mpsc::channel();
        let closer = {
            let server = server.clone();
            std::thread::spawn(move || closed.send(server.close()).unwrap())
        };
        noticed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            finished.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        control.release();
        canceller.join().unwrap();
        finished
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        closer.join().unwrap();
    }

    #[test]
    fn cancellation_cleanup_panic_preserves_failure_and_closes_new_admission() {
        let (server, factory) = fixture();
        factory.catalog_close_panics.store(true, Ordering::SeqCst);
        let call = begin(&server, "cancel-panic", "return 7;");
        server.cancel_call(&call);
        assert!(!call.stopping.load(Ordering::Acquire));
        let completed = server.run(&call);
        assert_eq!(completed.data["error"]["code"], "cancelled");
        assert_eq!(
            completed.data["shutdown_errors"][0]["code"],
            "shutdown_failed"
        );
        assert_eq!(
            reply(server.begin(json!("after-panic"), "apps", json!({}))).data["error"]["code"],
            "shutdown_failed"
        );
        assert_eq!(result(&server, "cancel-panic").data, completed.data);
        assert_eq!(factory.connects.load(Ordering::SeqCst), 0);
        assert!(server.close().is_err());
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
    fn receipt_capacity_slides_without_losing_the_response_owned_by_a_waiter() {
        let (server, factory) = fixture();
        let call = begin(&server, "old", "return 1;");
        let completed = server.finish_run(&call, Reply::plain(json!({
            "script":{"steps":[{"inventory":{"coverage":"supported subset","diagnostics":{"count":1}},
                "receipt":{"outcome":{"dispatch":"unknown","evidence":"unverified"}}}]},
            "value":"evictable result"
        })));
        assert!(receipt_size(&completed) > completed.data["script"].to_string().len());
        // Force the byte boundary without allocating sixteen MiB in a control test.
        server.state.lock().unwrap().records[0].receipt_bytes = RETENTION.receipt_bytes;
        let next = begin(&server, "new", "return 2;");
        let newest = server.finish_run(&next, Reply::plain(json!({"value":"new result"})));
        assert_eq!(server.state.lock().unwrap().records.len(), 1);
        assert_eq!(result(&server, "new").data, newest.data);
        let old = server.result(json!({"app":APP,"execution_id":call.execution_id}));
        assert_eq!(old.data["error"]["code"], "execution_unavailable");
        // A duplicate handler accepted before eviction still receives its original
        // response from the Call it already owns; no permanent tombstone is needed.
        assert_eq!(server.completed_reply(&call).data, completed.data);
        assert_eq!(
            reply(server.begin(
                json!("new"),
                "exec",
                json!({"app":APP,"source":"return 2;"})
            ))
            .data,
            newest.data
        );
        assert_eq!(factory.provider.0.actions.load(Ordering::SeqCst), 0);
        assert_eq!(factory.connects.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn reconciliation_cleanup_failure_blocks_admission_in_other_apps() {
        let (server, factory) = fixture();
        {
            let mut apps = factory.applications.lock().unwrap();
            let mut sibling = apps[0].clone();
            sibling.app.pid = 101;
            let mut other = apps[0].clone();
            other.app_id = "com.example.Other".into();
            other.app.pid = 102;
            apps.extend([sibling, other]);
        }
        let first = server.run(&begin(&server, "before", "const w=(await app.windows()).windows[0];return await app.observe({window_ref:w.window_ref});"));
        assert_eq!(first.data["status"], "completed");
        factory
            .applications
            .lock()
            .unwrap()
            .retain(|entry| entry.app.pid != 100);
        factory
            .provider
            .0
            .cleanup_fails
            .store(true, Ordering::SeqCst);
        let failed = server.run(&begin(&server, "reconcile", "return 1;"));
        assert!(failed.is_error, "{}", failed.data);
        assert!(failed.data.to_string().contains("binding_release_failed"));
        let blocked = reply(server.begin(
            json!("other"),
            "exec",
            json!({"app":"com.example.Other","source":"return 2;"}),
        ));
        assert!(blocked.is_error, "{}", blocked.data);
        assert!(blocked.data.to_string().contains("binding_release_failed"));
        assert_eq!(factory.connects.load(Ordering::SeqCst), 1);
        assert_eq!(factory.provider.0.actions.load(Ordering::SeqCst), 0);
        assert!(server.close().is_err());
    }

    #[test]
    fn stable_app_aggregates_instances_and_routes_identical_native_window_ids() {
        let (server, factory) = fixture();
        {
            let mut apps = factory.applications.lock().unwrap();
            let mut other = apps[0].clone();
            other.app.pid += 1;
            other.app.identity = AppIdentity("another-install-path".into());
            apps.push(other);
            // Duplicate catalog records do not create phantom instances.
            let duplicate = apps[0].clone();
            apps.push(duplicate);
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
        let call = begin(
            &server,
            "both",
            "state.count=7;state.windows=(await app.windows()).windows;return state.windows;",
        );
        let first = server.run(&call);
        assert_eq!(first.data["status"], "completed", "{}", first.data);
        let rows = first.data["value"]["result"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0]["window_ref"], rows[1]["window_ref"]);
        let source = "const w=state.windows[1].window_ref;const o=await app.observe({window_ref:w});await app.press({window_ref:w,observation_ref:o.observation_ref,element_ref:o.elements[0].element_ref});return {count:state.count,w};";
        let second = server.run(&begin(&server, "second", source));
        assert_eq!(second.data["status"], "completed", "{}", second.data);
        assert_eq!(
            factory.provider.0.action_targets.lock().unwrap()[0].app.pid,
            101
        );
        assert_eq!(factory.connects.load(Ordering::SeqCst), 1);

        // Exiting the first instance retains both B's ref and app JSON state.
        factory
            .applications
            .lock()
            .unwrap()
            .retain(|entry| entry.app.pid == 101);
        let surviving = server.run(&begin(&server, "surviving", source));
        assert_eq!(surviving.data["status"], "completed", "{}", surviving.data);
        assert_eq!(surviving.data["value"]["result"]["count"], 7);
        assert_eq!(
            surviving.data["value"]["result"]["w"],
            rows[1]["window_ref"]
        );
        assert_eq!(surviving.data["context_reset"], false);
        assert_eq!(surviving.data["state_reset"], false);
        assert_eq!(factory.provider.0.shutdowns.load(Ordering::SeqCst), 0);
        let gone = server.run(&begin(&server, "gone", "try {await app.observe({window_ref:state.windows[0].window_ref});return 'incorrect';} catch(e){return e.code;}"));
        assert_eq!(gone.data["value"]["result"], "target_changed");

        // A restarted sibling gets a fresh reference without resetting B.
        {
            let mut apps = factory.applications.lock().unwrap();
            let mut restarted = apps[0].clone();
            restarted.app.pid = 100;
            restarted.app.generation = 2;
            apps.insert(0, restarted);
        }
        let joined = server.run(&begin(
            &server,
            "joined",
            "return {count:state.count,windows:(await app.windows()).windows};",
        ));
        assert_eq!(joined.data["status"], "completed", "{}", joined.data);
        assert_eq!(joined.data["value"]["result"]["count"], 7);
        let new_rows = joined.data["value"]["result"]["windows"]
            .as_array()
            .unwrap();
        assert_eq!(new_rows.len(), 2);
        assert!(new_rows
            .iter()
            .any(|row| row["window_ref"] == rows[1]["window_ref"]));
        assert!(!new_rows
            .iter()
            .any(|row| row["window_ref"] == rows[0]["window_ref"]));
        assert_eq!(factory.connects.load(Ordering::SeqCst), 1);
        assert_eq!(result(&server, "both").data, first.data);
        server.close().unwrap();
    }
    #[test]
    fn completed_owner_replaces_a_stale_running_snapshot_and_exposes_internal_cleanup_errors() {
        for (fail, stopping) in [(false, false), (false, true), (true, false), (true, true)] {
            let (server, factory) = fixture();
            let call = begin(&server, "sampled", "return 7;");
            let app = factory.applications.lock().unwrap()[0].app.clone();
            let owner = Coordinator::new(factory.provider.clone(), app, "App".into());
            *call.owner.lock().unwrap() = Some(owner.clone());
            server.state.lock().unwrap().contexts.push_back(AppContext {
                app_id: APP.into(),
                instances: owner.instances(),
                script_state: owner.state.lock().unwrap().script_state.clone(),
                owner: Some(owner.clone()),
            });
            let admitted = owner.handle(Request::Submit(Submit {
                request_id: call.execution_id.clone().unwrap(),
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
            let before_cache = result(&server, "sampled");
            assert_eq!(before_cache.data["status"], "running");
            assert_eq!(before_cache.data["value"]["result"], 7);
            assert_eq!(before_cache.data["context_reset"], false);
            assert_eq!(before_cache.data["state_reset"], false);
            if fail {
                factory
                    .provider
                    .0
                    .shutdown_fails
                    .store(true, Ordering::SeqCst);
                assert!(owner.close().is_error);
            }
            let cancellation = stopping.then(|| {
                let (gate, control) = gate();
                *factory.catalog_close.lock().unwrap() = Some(gate);
                let canceller = {
                    let server = server.clone();
                    let call = call.clone();
                    std::thread::spawn(move || server.cancel_call(&call))
                };
                control.entered();
                (control, canceller)
            });
            let mut completed = server.finish_run(&call, admitted);
            assert_eq!(
                completed.data["status"],
                if stopping { "running" } else { "completed" }
            );
            assert_eq!(completed.data["value"]["result"], 7);
            if !fail {
                let mut same_phase = completed.data.clone();
                same_phase["status"] = json!("running");
                assert_eq!(before_cache.data, same_phase);
            }
            assert_eq!(result(&server, "sampled").data, completed.data);
            if let Some((control, canceller)) = cancellation {
                control.release();
                canceller.join().unwrap();
                // Refresh after cancellation must retain the original payload,
                // not overwrite it with an already-released owner payload.
                completed.data["status"] = json!("completed");
                assert_eq!(result(&server, "sampled").data, completed.data);
            }
            assert_eq!(result(&server, "sampled").data["status"], "completed");
            // The session owns the completed receipt; resident apps retain no
            // duplicate Job or script history after that handoff settles.
            assert!(owner.state.lock().unwrap().jobs.is_empty());
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
    let mut grouped: Vec<(String, String, Vec<AppInstance>)> = Vec::new();
    for entry in apps {
        if let Some((_, _, instances)) = grouped.iter_mut().find(|(id, _, _)| *id == entry.app_id) {
            if !instances.contains(&entry.app) {
                instances.push(entry.app);
            }
        } else {
            grouped.push((entry.app_id, entry.name, vec![entry.app]));
        }
    }
    grouped
        .into_iter()
        .map(|(id, name, instances)| {
            let count = instances.len();
            let mut row = json!({"app":id,"name":name});
            if count > 1 {
                row["instances"] = json!(count);
            }
            row
        })
        .collect()
}

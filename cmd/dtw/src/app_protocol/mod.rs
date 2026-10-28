//! App protocol coordination. Core checks the app binding for every native call.
use crate::{
    computer::*,
    runtime::{ActionReceipt, Runtime},
};
use base64::Engine;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
pub mod app_id;
mod description;
pub mod mcp;
mod mcp_io;
mod methods;
mod projection;
mod script_engine;
mod scripts;
pub mod server;
#[cfg(test)]
mod tests;
#[cfg(target_os = "macos")]
pub mod transport;
pub mod wire;

use wire::{Command, Request, Submit, WireError};

pub const MAX_REQUESTS: usize = 64;
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
const OBSERVATION_TTL: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct Reply {
    pub data: Value,
    pub image: Option<Arc<String>>,
    pub is_error: bool,
}
impl Reply {
    fn plain(data: Value) -> Self {
        Self {
            data,
            image: None,
            is_error: false,
        }
    }
    pub fn error(error: WireError) -> Self {
        Self {
            data: json!({"version":1,"error":error_json(&error)}),
            image: None,
            is_error: true,
        }
    }
    pub fn mcp(&self) -> Value {
        let mut content = vec![json!({"type":"text","text":self.data.to_string()})];
        if let Some(image) = &self.image {
            content.push(json!({"type":"image","data":image.as_str(),"mimeType":"image/png"}));
        }
        json!({"content":content,"structuredContent":self.data,"isError":self.is_error})
    }
}
fn error_json(error: &WireError) -> Value {
    let mut value = json!({"code":error.code,"message":error.message});
    if let Some(details) = &error.details {
        value["details"] = details.clone();
    }
    value
}
fn core_error(error: &Error) -> Value {
    error_json(&wire::core_error(error))
}
fn problem(code: &str, message: &str) -> Reply {
    Reply::error(WireError {
        details: None,
        code: code.into(),
        message: message.into(),
    })
}
fn reference(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4())
}
fn operation(command: &Command) -> &'static str {
    match command {
        Command::Script { .. } => "script",
        Command::Windows => "windows",
        Command::Observe => "observe",
        Command::Capture => "capture",
        Command::Press { .. } => "press",
        Command::Click { .. } => "click",
    }
}
fn is_action(command: &Command) -> bool {
    matches!(command, Command::Press { .. } | Command::Click { .. })
}

struct Snapshot {
    core: ObservationRef,
    elements: HashMap<String, ElementId>,
    public: String,
}
enum Publication {
    None,
    Windows(HashMap<String, WindowTarget>),
    Observation(Snapshot),
}
struct Completed {
    value: Option<Value>,
    image: Option<Arc<String>>,
    receipt: Option<ActionReceipt>,
    error: Option<WireError>,
    payload_bytes: usize,
    payload_expired: bool,
}
struct Work {
    completed: Completed,
    publication: Publication,
    state_update: Option<Value>,
}
impl Work {
    fn read_error(error: Error) -> Self {
        Self {
            completed: Completed {
                value: None,
                image: None,
                receipt: None,
                error: Some(wire::core_error(&error)),
                payload_bytes: 0,
                payload_expired: false,
            },
            publication: Publication::None,
            state_update: None,
        }
    }
    fn action(receipt: ActionReceipt) -> Self {
        let error = receipt.outcome.error.as_ref().map(wire::core_error);
        Self {
            completed: Completed {
                value: None,
                image: None,
                receipt: Some(receipt),
                error,
                payload_bytes: 0,
                payload_expired: false,
            },
            publication: Publication::None,
            state_update: None,
        }
    }
    fn rejected(command: &Command, error: Error) -> Self {
        if is_action(command) {
            Self::action(ActionReceipt {
                outcome: ActionOutcome::rejected(error),
                provider_outcome: None,
                interruption: None,
            })
        } else {
            Self::read_error(error)
        }
    }
}
struct Job {
    submitted: Submit,
    completed: Option<Completed>,
    script: Option<scripts::ScriptRun>,
    shutdown_error: Option<WireError>,
}
struct State {
    terminal: Option<Error>,
    shutdown_error: Option<WireError>,
    active: Option<String>,
    jobs: Vec<Job>,
    windows: HashMap<String, WindowTarget>,
    observation: Option<Snapshot>,
    // Committed logical app data outlives this native execution owner.
    // Lock order is Coordinator state, then this cell; never the reverse.
    script_state: Arc<Mutex<Value>>,
}
pub struct Coordinator<B: Backend> {
    runtime: Runtime<B>,
    app: AppInstance,
    coverage: &'static str,
    conditions: &'static [&'static str],
    app_name: String,
    app_ref: String,
    supported: Vec<Capability>,
    state: Mutex<State>,
    finished: Condvar,
    max_requests: usize,
    max_payload_bytes: usize,
}
impl<B: Backend + 'static> Coordinator<B> {
    pub fn new(backend: B, app: AppInstance, app_name: String) -> Arc<Self> {
        Self::with_script_state(backend, app, app_name, Arc::new(Mutex::new(json!({}))))
    }
    fn with_script_state(
        backend: B,
        app: AppInstance,
        app_name: String,
        script_state: Arc<Mutex<Value>>,
    ) -> Arc<Self> {
        let supported = Capability::ALL
            .into_iter()
            .filter(|c| backend.supports(*c))
            .collect();
        let coverage = backend.coverage();
        let conditions = backend.conditions();
        let owner = Arc::new(Self {
            runtime: Runtime::new(backend, app.clone(), OBSERVATION_TTL),
            app,
            coverage,
            conditions,
            app_name,
            app_ref: reference("app"),
            supported,
            state: Mutex::new(State {
                terminal: None,
                shutdown_error: None,
                active: None,
                jobs: Vec::new(),
                windows: HashMap::new(),
                observation: None,
                script_state,
            }),
            finished: Condvar::new(),
            max_requests: MAX_REQUESTS,
            max_payload_bytes: MAX_PAYLOAD_BYTES,
        });
        owner
    }
    pub fn handle(self: &Arc<Self>, request: Request) -> Reply {
        match request {
            Request::Describe => self.describe(),
            Request::Submit(submit) => self.submit(submit),
            Request::Result { request_id } => self.result(&request_id),
            Request::Cancel { request_id } => self.cancel(&request_id),
            Request::Close => self.close(),
        }
    }
    fn record_shutdown_error(
        &self,
        error: Option<Error>,
        active: Option<&str>,
    ) -> Option<WireError> {
        let error = error.map(|error| wire::core_error(&error))?;
        let mut state = self.state.lock().unwrap();
        let error = state.shutdown_error.get_or_insert(error).clone();
        if let Some(job) = state
            .jobs
            .iter_mut()
            .find(|job| Some(job.submitted.request_id.as_str()) == active)
        {
            job.shutdown_error.get_or_insert_with(|| error.clone());
            if let Some(run) = &mut job.script {
                run.shutdown_error.get_or_insert_with(|| error.clone());
            }
        }
        Some(error)
    }
    fn observe_cleanup_failure(&self, id: &str) {
        if let Some(error) = self.runtime.cleanup_error() {
            // Preserve cleanup independently from the already-produced native
            // result. A JS catch must not reopen execution or commit state.
            self.record_shutdown_error(Some(error), Some(id));
            let open = self.state.lock().unwrap().terminal.is_none();
            if open {
                self.close();
            }
        }
    }
    fn cleanup_terminal(&self) -> Option<WireError> {
        let (reason, active) = {
            let state = self.state.lock().unwrap();
            (state.terminal.clone(), state.active.clone())
        };
        let result = match reason {
            Some(Error::Cancelled) => self.runtime.cancel(),
            _ => self.runtime.close(),
        };
        self.record_shutdown_error(result.err(), active.as_deref())
    }
    fn checked_state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }
    fn describe(&self) -> Reply {
        let state = self.checked_state();
        let app = json!({"app_ref":self.app_ref,"name":self.app_name});
        Reply::plain(json!({
            "version":1,"app":app,
            "supported_capabilities":self.supported.iter().map(|c|c.as_str()).collect::<Vec<_>>(),
            "context":{"state":context_state(&state),"terminal_reason":state.terminal.as_ref().map(core_error),
                "shutdown_error":state.shutdown_error.as_ref().map(error_json)},
            "script":scripts::description(),
            "limits":{"max_requests":self.max_requests,"max_payload_bytes":self.max_payload_bytes,
                "observation_ttl_ms":OBSERVATION_TTL.as_millis(),
                "result_retention":"receipts until process exit; payloads may be evicted"},
            "coverage":self.coverage,"conditions":self.conditions,
            "semantics":{"cancel":"terminal for the context; obtain the original receipt with result",
                "retry":"same accepted request_id and command returns the existing record, never replays",
                "references":"opaque and context-local; windows and new observations retire prior observation",
                "effects":"dispatched does not mean confirmed; observe independently"}
        }))
    }
    fn submit(self: &Arc<Self>, submitted: Submit) -> Reply {
        let mut state = self.checked_state();
        if let Some(job) = state
            .jobs
            .iter()
            .find(|j| j.submitted.request_id == submitted.request_id)
        {
            return if job.submitted == submitted {
                self.job_reply(job)
            } else {
                problem(
                    "request_conflict",
                    "This accepted request_id already has a different command",
                )
            };
        }
        if let Some(error) = &state.terminal {
            return Reply::error(wire::core_error(error));
        }
        if state.active.is_some() {
            return Reply::error(wire::core_error(&Error::Busy));
        }
        if submitted.target.app_ref != self.app_ref {
            return Reply::error(wire::core_error(&Error::WrongApp));
        }
        if state.jobs.len() >= self.max_requests {
            return problem(
                "request_limit",
                "Context request capacity is exhausted; accepted IDs are never recycled",
            );
        }
        let id = submitted.request_id.clone();
        state.active = Some(id.clone());
        state.jobs.push(Job {
            submitted: submitted.clone(),
            completed: None,
            shutdown_error: None,
            script: match &submitted.command {
                Command::Script { timeout_ms, .. } => Some(scripts::ScriptRun::new(
                    Instant::now() + Duration::from_millis(*timeout_ms),
                )),
                _ => None,
            },
        });
        let accepted = self.job_reply(state.jobs.last().unwrap());
        let owner = self.clone();
        // The reservation exists before any thread can dispatch. No execution queue.
        let launch = std::thread::Builder::new()
            .name("dtw-app-call".into())
            .spawn(move || {
                let work = match catch_unwind(AssertUnwindSafe(|| owner.execute(&submitted))) {
                    Ok(work) => work,
                    Err(_) => {
                        if matches!(&submitted.command, Command::Script { .. }) {
                            owner.stop_script(&submitted.request_id, Error::Closed);
                            owner.abandon_script_step(&submitted.request_id);
                        } else {
                            let _ = owner.close();
                        }
                        let error =
                            Error::BackendFailed("Execution panicked; context closed".into());
                        if is_action(&submitted.command) {
                            Work::action(ActionReceipt {
                                outcome: ActionOutcome::unknown(error),
                                provider_outcome: None,
                                interruption: Some(Error::Closed),
                            })
                        } else {
                            Work::read_error(error)
                        }
                    }
                };
                owner.complete(&submitted.request_id, work);
            });
        if let Err(error) = launch {
            let work = Work::rejected(
                &state.jobs.last().unwrap().submitted.command,
                Error::BackendFailed(format!("Cannot start execution: {error}")),
            );
            state.jobs.last_mut().unwrap().completed = Some(work.completed);
            state.active = None;
            self.finished.notify_all();
            return self.job_reply(state.jobs.last().unwrap());
        }
        accepted
    }
    fn result(&self, id: &str) -> Reply {
        let state = self.state.lock().unwrap();
        state
            .jobs
            .iter()
            .find(|j| j.submitted.request_id == id)
            .map(|job| self.job_reply(job))
            .unwrap_or_else(|| {
                problem(
                    "unknown_request",
                    "No accepted request has this ID in this context",
                )
            })
    }
    // The public session ledger takes ownership of evictable results. Keeping
    // a second payload cache in every resident app would multiply its memory cap.
    fn release_result_payload(&self, id: &str) {
        let mut state = self.state.lock().unwrap();
        if let Some(done) = state
            .jobs
            .iter_mut()
            .find(|job| job.submitted.request_id == id)
            .and_then(|job| job.completed.as_mut())
        {
            done.value = None;
            done.image = None;
            done.payload_bytes = 0;
            done.payload_expired = true;
        }
    }
    fn job_reply(&self, job: &Job) -> Reply {
        let done = job.completed.as_ref();
        let error = done.and_then(|d| d.error.as_ref());
        let mut data = json!({"version":1,"request_id":job.submitted.request_id,"app_ref":self.app_ref,
                "operation":operation(&job.submitted.command),
                "status":if done.is_some() {"completed"} else {"running"},
                "value":done.and_then(|d|d.value.as_ref()),
                "error":error.map(error_json),
                "receipt":done.and_then(|d|d.receipt.as_ref()).map(wire::receipt),
                "payload_expired":done.is_some_and(|d|d.payload_expired)});
        if let Some(script) = &job.script {
            data["script"] = script.json();
        }
        if let Some(error) = &job.shutdown_error {
            data["shutdown_error"] = error_json(error);
        }
        Reply {
            data,
            image: done.and_then(|d| d.image.clone()),
            is_error: error.is_some() || job.shutdown_error.is_some(),
        }
    }
    fn execute(&self, submitted: &Submit) -> Work {
        if matches!(&submitted.command, Command::Script { .. }) {
            return self.execute_script(submitted);
        }
        self.execute_atomic(submitted, Instant::now() + Duration::from_secs(15))
    }
    // Called under the outer job reservation, including every script Host Call.
    fn execute_atomic(&self, submitted: &Submit, deadline: Instant) -> Work {
        let work = self.execute_atomic_inner(submitted, deadline);
        self.observe_cleanup_failure(&submitted.request_id);
        work
    }
    fn execute_atomic_inner(&self, submitted: &Submit, deadline: Instant) -> Work {
        if submitted.command == Command::Windows {
            return match self.runtime.windows_until(deadline) {
                Err(error) => Work::read_error(error),
                Ok(inventory) => {
                    let windows = inventory.windows;
                    if windows.len() > 256 {
                        return Work::read_error(Error::BackendFailed(
                            "Window inventory exceeds protocol limit".into(),
                        ));
                    }
                    let mut bound = HashMap::new();
                    let rows:Vec<_>=windows.into_iter().map(|window| {
                        let key=reference("window");
                        let row=json!({"window_ref":key,"title":window.title,"visible":window.visible});
                        bound.insert(key,window.target); row
                    }).collect();
                    Work {
                        completed: Completed {
                            value: Some({
                                let mut value = wire::inventory_report(&inventory.report, true);
                                value["windows"] = json!(rows);
                                value
                            }),
                            image: None,
                            receipt: None,
                            error: None,
                            payload_bytes: 0,
                            payload_expired: false,
                        },
                        publication: Publication::Windows(bound),
                        state_update: None,
                    }
                }
            };
        }
        let target = {
            let state = self.state.lock().unwrap();
            if let Some(error) = &state.terminal {
                return Work::rejected(&submitted.command, error.clone());
            }
            submitted
                .target
                .window_ref
                .as_ref()
                .and_then(|id| state.windows.get(id))
                .cloned()
        };
        let Some(target) = target else {
            return Work::rejected(&submitted.command, Error::TargetChanged);
        };
        match &submitted.command {
            Command::Observe | Command::Capture => {
                let read = if submitted.command == Command::Observe {
                    self.runtime.observe_until(&target, deadline)
                } else {
                    self.runtime.capture_until(&target, deadline)
                };
                match read {
                    Err(error) => Work::read_error(error),
                    Ok(observation) => self.observation(submitted, observation),
                }
            }
            Command::Press {
                observation_ref,
                element_ref,
            } => {
                let bound = {
                    let state = self.state.lock().unwrap();
                    if let Some(error) = &state.terminal {
                        return Work::rejected(&submitted.command, error.clone());
                    }
                    state
                        .observation
                        .as_ref()
                        .filter(|o| o.public == *observation_ref)
                        .and_then(|o| {
                            o.elements
                                .get(element_ref)
                                .map(|element| (o.core, *element))
                        })
                };
                match bound {
                    Some((observation, element)) => Work::action(self.runtime.press_until(
                        &target,
                        observation,
                        element,
                        deadline,
                    )),
                    None => Work::rejected(&submitted.command, Error::InvalidObservation),
                }
            }
            Command::Click {
                observation_ref,
                x,
                y,
            } => {
                let bound = {
                    let state = self.state.lock().unwrap();
                    if let Some(error) = &state.terminal {
                        return Work::rejected(&submitted.command, error.clone());
                    }
                    state
                        .observation
                        .as_ref()
                        .filter(|o| o.public == *observation_ref)
                        .map(|o| o.core)
                };
                match bound {
                    Some(observation) => Work::action(self.runtime.click_until(
                        &target,
                        observation,
                        PixelPoint { x: *x, y: *y },
                        deadline,
                    )),
                    None => Work::rejected(&submitted.command, Error::InvalidObservation),
                }
            }
            Command::Windows | Command::Script { .. } => unreachable!(),
        }
    }
    fn observation(&self, submitted: &Submit, observation: Observation) -> Work {
        let public = reference("observation");
        let mut elements = HashMap::new();
        let rows: Vec<_> = observation
            .elements
            .into_iter()
            .map(|element| {
                let id = reference("element");
                let row =
                    json!({"element_ref":id,"label":element.label,"can_press":element.can_press,"role":element.role,"name":element.name,"value":element.value,"enabled":element.enabled,"actions":element.actions});
                elements.insert(id, element.id);
                row
            })
            .collect();
        let mut value = json!({"window_ref":submitted.target.window_ref,"observation_ref":public,
            "kind":match observation.kind {ObservationKind::Semantic=>"semantic",ObservationKind::Pixels=>"pixels"}});
        let image = if let Some(frame) = observation.image {
            value["image"] = json!({"width":frame.width,"height":frame.height,"coordinate_space":"image_pixels"});
            Some(Arc::new(
                base64::engine::general_purpose::STANDARD.encode(frame.png),
            ))
        } else {
            value["elements"] = json!(rows);
            None
        };
        Work {
            completed: Completed {
                value: Some(value),
                image,
                receipt: None,
                error: None,
                payload_bytes: 0,
                payload_expired: false,
            },
            publication: Publication::Observation(Snapshot {
                core: observation.reference,
                elements,
                public,
            }),
            state_update: None,
        }
    }
    fn complete(&self, id: &str, mut work: Work) {
        self.observe_cleanup_failure(id);
        // Recheck immediately at the state commit boundary. A script that
        // finishes on the deadline must not commit state or retain an idle worker.
        let mut state = loop {
            let state = self.state.lock().unwrap();
            let script = state
                .jobs
                .iter()
                .find(|j| j.submitted.request_id == id)
                .and_then(|j| j.script.as_ref());
            if let Some(script) = script {
                if state.terminal.is_none() && Instant::now() >= script.deadline {
                    drop(state);
                    let reason = Error::TimedOut;
                    self.stop_script(id, reason.clone());
                    work.completed.error = Some(wire::core_error(&reason));
                    work.state_update = None;
                    continue;
                }
            }
            break state;
        };
        if let Some(next) = work.state_update.take() {
            if state.terminal.is_none() && work.completed.error.is_none() {
                *state.script_state.lock().unwrap() = next;
                state
                    .jobs
                    .iter_mut()
                    .find(|j| j.submitted.request_id == id)
                    .unwrap()
                    .script
                    .as_mut()
                    .unwrap()
                    .state_committed = true;
            } else if work.completed.error.is_none() {
                work.completed.error = state.terminal.as_ref().map(wire::core_error);
            }
        }
        apply_publication(&mut state, work.publication);
        work.completed.payload_bytes = work
            .completed
            .value
            .as_ref()
            .map_or(0, |v| v.to_string().len())
            + work.completed.image.as_ref().map_or(0, |i| i.len());
        state
            .jobs
            .iter_mut()
            .find(|j| j.submitted.request_id == id)
            .unwrap()
            .completed = Some(work.completed);
        let mut bytes: usize = state
            .jobs
            .iter()
            .filter_map(|j| j.completed.as_ref())
            .map(|c| c.payload_bytes)
            .sum();
        for job in &mut state.jobs {
            if bytes <= self.max_payload_bytes {
                break;
            }
            if let Some(done) = &mut job.completed {
                if done.payload_bytes > 0 {
                    bytes -= done.payload_bytes;
                    done.value = None;
                    done.image = None;
                    done.payload_bytes = 0;
                    done.payload_expired = true;
                }
            }
        }
        state.active = None;
        self.finished.notify_all();
    }
    fn cancel(&self, id: &str) -> Reply {
        {
            let mut state = self.checked_state();
            if state.active.as_deref() != Some(id)
                || (state.terminal.is_some() && !matches!(state.terminal, Some(Error::Cancelled)))
            {
                return Reply::plain(json!({"version":1,"request_id":id,"cancel_requested":false,
                    "context_state":context_state(&state),"terminal_reason":state.terminal.as_ref().map(core_error),
                    "shutdown_error":state.shutdown_error.as_ref().map(error_json)}));
            }
            // Compare the active ID and close admission atomically. A stale cancel
            // can never stop a new job admitted between this check and Core.cancel.
            if state.terminal.is_none() {
                state.terminal = Some(Error::Cancelled);
            }
            scripts::cancel_active(&state);
            state.windows.clear();
            state.observation = None;
        }
        let error = self.cleanup_terminal();
        Reply {
            data: json!({"version":1,"request_id":id,"cancel_requested":true,
            "context_state":"cancelled","terminal_reason":core_error(&Error::Cancelled),
            "shutdown_error":error.as_ref().map(error_json)}),
            image: None,
            is_error: error.is_some(),
        }
    }
    pub fn close(&self) -> Reply {
        {
            let mut state = self.checked_state();
            if state.terminal.is_none() {
                state.terminal = Some(Error::Closed);
            }
            if !matches!(state.terminal, Some(Error::TimedOut)) {
                scripts::cancel_active(&state);
            }
            state.windows.clear();
            state.observation = None;
        }
        self.cleanup_terminal();
        let state = self.state.lock().unwrap();
        Reply {
            data: json!({"version":1,"context_state":context_state(&state),
            "terminal_reason":state.terminal.as_ref().map(core_error),
            "shutdown_error":state.shutdown_error.as_ref().map(error_json)}),
            image: None,
            is_error: state.shutdown_error.is_some(),
        }
    }
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap();
        while state.active.is_some() {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self.finished.wait_timeout(state, left).unwrap().0;
        }
        true
    }
}
fn apply_publication(state: &mut State, publication: Publication) {
    if state.terminal.is_some() {
        return;
    }
    match publication {
        Publication::Windows(windows) => {
            state.windows = windows;
            state.observation = None;
        }
        Publication::Observation(observation) => state.observation = Some(observation),
        Publication::None => {}
    }
}

fn context_state(state: &State) -> &'static str {
    match state.terminal {
        Some(Error::Cancelled) => "cancelled",
        Some(_) => "closed",
        None => "open",
    }
}

/// Run the app-scoped transport. Unsupported platforms never start another backend.
pub fn run(args: &[String]) -> Result<(), WireError> {
    #[cfg(target_os = "macos")]
    {
        transport::run(args)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = args;
        Err(WireError {
            details: None,
            code: "unsupported_platform".into(),
            message: "Native app operations currently require macOS".into(),
        })
    }
}

/// Generated from the same schemas that apps exposes. No native startup.
pub fn script_api_types() -> String {
    description::typescript()
}

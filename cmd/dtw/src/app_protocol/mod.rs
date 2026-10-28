//! App protocol coordination. Core remains the authority for every native call.
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
mod description;
pub mod grant;
pub mod mcp;
mod script_engine;
mod scripts;
#[cfg(test)]
mod tests;
#[cfg(target_os = "macos")]
pub mod transport;
pub mod wire;

use grant::HostGrant;
use wire::{Command, Request, Submit, WireError};

pub const MAX_REQUESTS: usize = 64;
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
const OBSERVATION_TTL: Duration = Duration::from_secs(15);

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
}
struct State {
    terminal: Option<Error>,
    shutdown_error: Option<WireError>,
    active: Option<String>,
    jobs: Vec<Job>,
    windows: HashMap<String, WindowTarget>,
    observation: Option<Snapshot>,
    script_state: Value,
}
pub struct Coordinator<B: Backend> {
    runtime: Runtime<B>,
    authority: HostGrant,
    app_name: String,
    app_ref: String,
    supported: Vec<Capability>,
    state: Mutex<State>,
    finished: Condvar,
    max_requests: usize,
    max_payload_bytes: usize,
}
impl<B: Backend + 'static> Coordinator<B> {
    pub fn new(backend: B, authority: HostGrant, app_name: String) -> Arc<Self> {
        let supported = [
            Capability::Observe,
            Capability::Press,
            Capability::Capture,
            Capability::Click,
        ]
        .into_iter()
        .filter(|c| backend.supports(*c))
        .collect();
        let owner = Arc::new(Self {
            runtime: Runtime::new(backend, authority.core_grant(), OBSERVATION_TTL),
            authority,
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
                script_state: json!({}),
            }),
            finished: Condvar::new(),
            max_requests: MAX_REQUESTS,
            max_payload_bytes: MAX_PAYLOAD_BYTES,
        });
        owner.expire_if_needed();
        owner
    }
    pub fn handle(self: &Arc<Self>, request: Request) -> Reply {
        self.expire_if_needed();
        match request {
            Request::Describe => self.describe(),
            Request::Submit(submit) => self.submit(submit),
            Request::Result { request_id } => self.result(&request_id),
            Request::Cancel { request_id } => self.cancel(&request_id),
            Request::Close => self.close(),
        }
    }
    /// Check idle expiry without renewing the Host's immutable deadline.
    /// Stdio calls this from its lifecycle watcher; embedded Hosts may call it
    /// directly. Every protocol entry also checks before reading or admitting.
    pub fn expire_if_needed(&self) {
        if Instant::now() < self.authority.expires_at {
            return;
        }
        let active = {
            let mut state = self.state.lock().unwrap();
            if state.terminal.is_some() {
                return;
            }
            state.terminal = Some(Error::GrantExpired);
            state.windows.clear();
            state.observation = None;
            // The VM shares this deadline. Its cancellation flag must not turn
            // expiry into an explicit cancellation.
            state.active.clone()
        };
        self.record_shutdown_error(self.runtime.expire().err(), active.as_deref());
    }
    fn record_shutdown_error(
        &self,
        error: Option<Error>,
        active: Option<&str>,
    ) -> Option<WireError> {
        let error = error.map(|error| wire::core_error(&error))?;
        let mut state = self.state.lock().unwrap();
        state.shutdown_error = Some(error.clone());
        if let Some(run) = state
            .jobs
            .iter_mut()
            .find(|job| Some(job.submitted.request_id.as_str()) == active)
            .and_then(|job| job.script.as_mut())
        {
            run.shutdown_error = Some(error.clone());
        }
        Some(error)
    }
    fn cleanup_terminal(&self) -> Option<WireError> {
        let (reason, active) = {
            let state = self.state.lock().unwrap();
            (state.terminal.clone(), state.active.clone())
        };
        let result = match reason {
            Some(Error::GrantExpired) => self.runtime.expire(),
            Some(Error::Cancelled) => self.runtime.cancel(),
            _ => self.runtime.close(),
        };
        self.record_shutdown_error(result.err(), active.as_deref())
    }
    fn checked_state(&self) -> std::sync::MutexGuard<'_, State> {
        loop {
            let state = self.state.lock().unwrap();
            if state.terminal.is_some() || Instant::now() < self.authority.expires_at {
                return state;
            }
            drop(state);
            self.expire_if_needed();
        }
    }
    fn describe(&self) -> Reply {
        let state = self.checked_state();
        let mut app = grant::app_json(&self.authority.app);
        app["app_ref"] = json!(self.app_ref);
        app["name"] = json!(self.app_name);
        Reply::plain(json!({
            "version":1,"app":app,
            "grant":{"grant_id":self.authority.grant_id,
                "capabilities":grant::capabilities_json(&self.authority.capabilities),
                "expires_at_unix_ms":self.authority.expires_at_unix_ms},
            "supported_capabilities":grant::capabilities_json(&self.supported),
            "context":{"state":context_state(&state),"terminal_reason":state.terminal.as_ref().map(core_error),
                "shutdown_error":state.shutdown_error.as_ref().map(error_json),
                "expiry":"absolute Host deadline; stdio lifecycle watcher polls idle expiry every 50 ms, subject to scheduling and cleanup; embedded coordinators check on entry or expire_if_needed; no renewal"},
            "script":scripts::description(),
            "limits":{"max_requests":self.max_requests,"max_payload_bytes":self.max_payload_bytes,
                "observation_ttl_ms":OBSERVATION_TTL.as_millis(),
                "result_retention":"receipts until process exit; payloads may be evicted"},
            "coverage":"same-PID standard layer-0 AX windows; no sheet/menu/helper routing or automatic focus",
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
            script: match &submitted.command {
                Command::Script { timeout_ms, .. } => Some(scripts::ScriptRun::new(
                    (Instant::now() + Duration::from_millis(*timeout_ms))
                        .min(self.authority.expires_at),
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
        Reply {
            data,
            image: done.and_then(|d| d.image.clone()),
            is_error: error.is_some(),
        }
    }
    fn execute(&self, submitted: &Submit) -> Work {
        if matches!(&submitted.command, Command::Script { .. }) {
            return self.execute_script(submitted);
        }
        self.execute_atomic(submitted, self.authority.expires_at)
    }
    // Called under the outer job reservation, including every script Host Call.
    fn execute_atomic(&self, submitted: &Submit, deadline: Instant) -> Work {
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
                    json!({"element_ref":id,"label":element.label,"can_press":element.can_press});
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
        self.expire_if_needed();
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
                    let reason = if Instant::now() >= self.authority.expires_at {
                        Error::GrantExpired
                    } else {
                        Error::TimedOut
                    };
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
                state.script_state = next;
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
        self.expire_if_needed();
        {
            let mut state = self.checked_state();
            if state.terminal.is_none() {
                state.terminal = Some(Error::Closed);
            }
            if !matches!(state.terminal, Some(Error::GrantExpired | Error::TimedOut)) {
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

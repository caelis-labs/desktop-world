//! Narrow macOS provider. All native objects and SCK clients belong to its worker.
use super::process::{StopReason, TransportError, TransportErrorKind, Worker};
use crate::computer::*;
use base64::Engine;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
mod associated_proof;
mod cua_input;
mod cursor_session;
mod local_socket;
mod overlay;
mod pointer_presentation;
mod scroll_guard;
pub use cursor_session::CursorSession;
pub use overlay::worker_entry as overlay_worker_entry;
mod worker;
pub use worker::worker_entry;

// Normal focus return may need multiple bounded AX reads during a modal
// transition. Emergency shutdown retains its separate, shorter drain budget.
const NORMAL_FOCUS_RETURN_BUDGET: Duration = Duration::from_secs(2);
const FOCUS_RETURN_TRANSPORT_MARGIN: Duration = Duration::from_millis(100);

fn focus_return_deadline_ms(now: u64, call_deadline: Option<u64>) -> u64 {
    let capped = now.saturating_add(NORMAL_FOCUS_RETURN_BUDGET.as_millis() as u64);
    call_deadline.map_or(capped, |deadline| deadline.min(capped))
}

fn focus_return_request(now: u64, call_deadline: Option<u64>) -> Value {
    let mut request = json!({
        "operation": "release_focus",
        "deadline_unix_ms": focus_return_deadline_ms(now, call_deadline),
        "ownership_cap_unix_ms": focus_return_deadline_ms(now, None),
    });
    if let Some(deadline) = call_deadline {
        request["caller_deadline_unix_ms"] = json!(deadline);
    }
    request
}

// Private capture-only classification. Neither stale references nor arbitrary
// native errors are eligible for another read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureGeometryStage {
    Scene,
    Resident,
}
impl CaptureGeometryStage {
    fn reply(self) -> Value {
        let stage = match self {
            Self::Scene => "scene",
            Self::Resident => "resident",
        };
        json!({"error":"capture_geometry_changed",
            "message":format!("Window geometry changed during {stage} capture validation; no image was published"),
            "details":{"capture_stage":stage}})
    }
    fn from_reply(reply: &Value) -> Option<Self> {
        if reply["error"] != "capture_geometry_changed" || reply.get("dispatch").is_some() {
            return None;
        }
        match reply["details"]["capture_stage"].as_str()? {
            "scene" => Some(Self::Scene),
            "resident" => Some(Self::Resident),
            _ => None,
        }
    }
}

fn fresh_capture_reply(
    target: &WindowTarget,
    call: &CallContext<'_>,
    mut capture: impl FnMut(Value, &CallContext<'_>) -> Result<Value, (Error, bool)>,
) -> Result<Value, Error> {
    let request = json!({"operation":"capture","target":target_json(target)});
    for attempt in 0..2 {
        if call.remaining()?.is_zero() {
            return Err(Error::TimedOut);
        }
        let reply = capture(request.clone(), call).map_err(|(error, _)| error)?;
        if attempt == 0 && CaptureGeometryStage::from_reply(&reply).is_some() {
            // Discard this entire frame. The next invocation binds and validates
            // the same target from scratch under the original live budget.
            continue;
        }
        if let Some(error) = reply_error(&reply) {
            return Err(error);
        }
        return Ok(reply);
    }
    unreachable!("at most one fresh geometry read")
}

pub struct AppInfo {
    pub app_id: String,
    pub app: AppInstance,
    pub name: String,
}
pub struct NativeBackend {
    worker: Arc<Worker>,
    scene: SceneOwner,
    stamp: ProviderStamp,
    cleanup_failure: Mutex<Option<Error>>,
    input_policy: InputPolicy,
    focus_pending: AtomicBool,
    cursor: Arc<CursorSession>,
    cursor_operation: AtomicU64,
}

#[derive(Default)]
struct SceneState {
    closed: bool,
    worker: Option<Arc<Worker>>,
    failure: Option<Error>,
    guard: Option<Arc<scroll_guard::Owner>>,
}

/// Owns a short-lived child of the MCP process, never a child of its AX worker.
/// The slot is retained until cleanup succeeds; stopping cannot miss a spawn.
#[derive(Default)]
struct SceneOwner {
    state: Mutex<SceneState>,
}
impl SceneOwner {
    fn cleanup_error(&self) -> Option<Error> {
        self.state.lock().unwrap().failure.clone()
    }

    fn check(&self) -> Result<(), Error> {
        let state = self.state.lock().unwrap();
        if let Some(error) = &state.failure {
            Err(error.clone())
        } else if state.closed {
            Err(Error::Closed)
        } else {
            Ok(())
        }
    }

    fn start(
        &self,
        spawn: impl FnOnce() -> Result<Worker, Error>,
    ) -> Result<SceneFlight<'_>, Error> {
        let mut state = self.state.lock().unwrap();
        if let Some(error) = &state.failure {
            return Err(error.clone());
        }
        if state.closed {
            return Err(Error::Closed);
        }
        if state.worker.is_some() {
            return Err(Error::Busy);
        }
        // Spawn and publication share the close lock. The spawned process is
        // either recorded here or returned as an owned transport spawn failure.
        let worker = Arc::new(spawn()?);
        state.worker = Some(worker.clone());
        Ok(SceneFlight {
            owner: self,
            worker,
            finished: false,
        })
    }

    fn finish(&self, worker: &Arc<Worker>) -> Result<(), Error> {
        if let Some(guard) = self.state.lock().unwrap().guard.as_ref() {
            guard.invalidate();
        }
        let result = worker.stop().map_err(transport_error);
        let mut state = self.state.lock().unwrap();
        if let Err(error) = &result {
            state.failure.get_or_insert_with(|| error.clone());
            state.closed = true;
        } else if state
            .worker
            .as_ref()
            .is_some_and(|owned| Arc::ptr_eq(owned, worker))
        {
            state.worker = None;
            state.guard = None;
        }
        result
    }

    fn close(&self) -> Result<(), Error> {
        let worker = {
            let mut state = self.state.lock().unwrap();
            state.closed = true;
            if let Some(guard) = &state.guard {
                guard.invalidate();
            }
            state.worker.clone()
        };
        if let Some(worker) = worker {
            self.finish(&worker)?;
        }
        let state = self.state.lock().unwrap();
        state.failure.clone().map_or(Ok(()), Err)
    }
    fn attach_guard(
        &self,
        worker: &Arc<Worker>,
        guard: Arc<scroll_guard::Owner>,
    ) -> Result<(), Error> {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            guard.invalidate();
            return Err(Error::Closed);
        }
        if !state
            .worker
            .as_ref()
            .is_some_and(|owned| Arc::ptr_eq(owned, worker))
        {
            guard.invalidate();
            return Err(Error::Busy);
        }
        state.guard = Some(guard);
        Ok(())
    }
}

struct SceneFlight<'a> {
    owner: &'a SceneOwner,
    worker: Arc<Worker>,
    finished: bool,
}
impl SceneFlight<'_> {
    fn finish(mut self) -> Result<(), Error> {
        self.finished = true;
        self.owner.finish(&self.worker)
    }
}
impl Drop for SceneFlight<'_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.owner.finish(&self.worker);
        }
    }
}

// Preserve successful last-pointer feedback, but retire this exact generation
// on any RPC failure. A failed older app cannot hide another app's newer marker.
struct CursorAttempt<'a> {
    cursor: &'a CursorSession,
    operation: Option<u64>,
}
impl Drop for CursorAttempt<'_> {
    fn drop(&mut self) {
        if let Some(operation) = self.operation {
            self.cursor.hide(operation);
        }
    }
}

pub struct NativeBinding {
    worker: Arc<Worker>,
    semantic: Option<String>,
    pixels: Option<(Value, u32, u32)>,
}
impl Drop for NativeBinding {
    fn drop(&mut self) {
        if let Some(token) = self.semantic.take() {
            // Cleanup only releases objects. It cannot invoke input, and a failed
            // worker is never revived.
            let until = Instant::now() + Duration::from_millis(300);
            let _ = self.worker.call(
                json!({"operation":"release","binding":token,"request_id":unique_generation()})
                    .to_string()
                    .as_bytes(),
                || {
                    let left = until.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        Err(StopReason::TimedOut)
                    } else {
                        Ok(left)
                    }
                },
            );
        }
    }
}
const COVERAGE: &str =
    "application surfaces and native service dialogs with a proven public-AX relationship; AX semantics remain an exact-surface capability";
const CONDITIONS: &[&str] = &[
    "semantic and input operations require macOS Accessibility permission",
    "capture and click require macOS Screen Recording permission",
    "input prefers directed background delivery; a side-effect-free preflight decline may use temporary foreground focus",
    "foreground coordinate fallback requires a visible, unobstructed point in the captured window",
    "semantic scopes include retained subtrees, capture-pixel regions and the app menu; region geometry does not prove pixel visibility",
    "readText requires native textual content and preserves one content revision across UTF-16 pages; titles and descriptions are not document text",
    "input requires an exact app-associated AX surface and current modal target; proven cross-process service surfaces use foreground input",
    "borrowed foreground is returned only while still owned; uncertain input is never rerouted or replayed",
];
impl NativeBackend {
    fn supports_capability(capability: Capability) -> bool {
        matches!(
            capability,
            Capability::Windows
                | Capability::Observe
                | Capability::Press
                | Capability::Capture
                | Capability::Click
                | Capability::Focus
                | Capability::TypeText
                | Capability::Keypress
                | Capability::Scroll
                | Capability::SetValue
                | Capability::ReadText
        )
    }
    pub fn description() -> Value {
        json!({"provider":"dtw-macos-native", "coverage":COVERAGE,"conditions":CONDITIONS,
            "supported_capabilities":Capability::ALL.into_iter().filter(|c|Self::supports_capability(*c)).map(Capability::as_str).collect::<Vec<_>>(),
            "supported_observe_scopes":ObserveScopeKind::ALL.map(ObserveScopeKind::as_str),
            "supported_window_wait_states":WindowWaitState::ALL.map(WindowWaitState::as_str)})
    }

    pub fn spawn() -> Result<Self, Error> {
        Self::spawn_with_policy(InputPolicy::Auto)
    }

    pub fn spawn_with_policy(input_policy: InputPolicy) -> Result<Self, Error> {
        Self::spawn_with_cursor(input_policy, Arc::new(CursorSession::default()))
    }

    pub fn spawn_with_cursor(
        input_policy: InputPolicy,
        cursor: Arc<CursorSession>,
    ) -> Result<Self, Error> {
        let exe = std::env::current_exe().map_err(failed)?;
        let worker = Worker::spawn(&exe, &["--dtw-native-worker"]).map_err(transport_error)?;
        Ok(Self {
            worker: Arc::new(worker),
            scene: SceneOwner::default(),
            cleanup_failure: Mutex::new(None),
            focus_pending: AtomicBool::new(false),
            cursor,
            cursor_operation: AtomicU64::new(0),
            input_policy,
            stamp: ProviderStamp {
                id: "dtw-macos-native".into(),
                generation: unique_generation(),
            },
        })
    }
    /// Low-permission process metadata only. AX window discovery is in Runtime.
    pub fn catalog(&self, timeout: Duration) -> Result<Vec<AppInfo>, Error> {
        let deadline = Instant::now() + timeout;
        let poll = || {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                Err(Error::TimedOut)
            } else {
                Ok(left)
            }
        };
        let reply = self.read_rpc(json!({"operation":"catalog"}), &CallContext { poll: &poll })?;
        array(&reply, "apps")?
            .iter()
            .map(|entry| {
                Ok(AppInfo {
                    app_id: string(entry, "bundle_id")?.into(),
                    app: parse_app(&entry["app"])?,
                    name: string(entry, "name")?.into(),
                })
            })
            .collect()
    }
    fn rpc(&self, request: Value, call: &CallContext<'_>) -> Result<Value, (Error, bool)> {
        self.scene.check().map_err(|error| (error, false))?;
        self.rpc_on(&self.worker, request, call)
    }

    fn rpc_on(
        &self,
        worker: &Worker,
        request: Value,
        call: &CallContext<'_>,
    ) -> Result<Value, (Error, bool)> {
        self.rpc_on_guarded(worker, request, call, None)
    }
    fn rpc_on_guarded(
        &self,
        worker: &Worker,
        mut request: Value,
        call: &CallContext<'_>,
        guard: Option<(&Arc<scroll_guard::Owner>, &Value)>,
    ) -> Result<Value, (Error, bool)> {
        let mut cursor_attempt = CursorAttempt {
            cursor: &self.cursor,
            operation: None,
        };
        let request_id = unique_generation();
        request["request_id"] = json!(request_id);
        request["input_policy"] = json!(self.input_policy.as_str());
        if matches!(request["operation"].as_str(), Some("click" | "scroll")) {
            // scene_rpc already validated the retained target before this point.
            // Presentation never grants input admission or changes its outcome.
            if let Some(endpoint) = self.cursor.begin(call) {
                cursor_attempt.operation = Some(endpoint.operation);
                self.cursor_operation
                    .store(endpoint.operation, Ordering::Release);
                request["_cursor_endpoint"] = json!(endpoint);
            }
            let left = call.remaining().map_err(|error| (error, false))?;
            let mut candidates = overlay::discover_candidates(Instant::now() + left);
            if let Some(identity) = self.cursor.identity() {
                if !candidates.contains(&identity) {
                    candidates.push(identity);
                }
            }
            request["_overlay_candidates"] = json!(candidates);
        }
        let left = call
            .remaining()
            .map_err(|e| (e, false))?
            .min(Duration::from_secs(15));
        let deadline = Instant::now() + left;
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| (failed(e), false))?
            + left;
        request["deadline_unix_ms"] = json!(wall.as_millis() as u64);
        let stop = std::sync::Mutex::new(None);
        let bytes = worker
            .call(request.to_string().as_bytes(), || {
                if let Some((owner, validation)) = guard {
                    let query = match owner.pending() {
                        Ok(query) => query,
                        Err(error) => {
                            owner.invalidate();
                            *stop.lock().unwrap() = Some(failed(error));
                            return Err(StopReason::Cancelled);
                        }
                    };
                    if let Some(query) = query {
                        // pending() returns an owned request: no pump/channel
                        // mutex remains held while entering this nested RPC.
                        let poll = || {
                            let left = call.remaining().and_then(|left| {
                                let left =
                                    left.min(deadline.saturating_duration_since(Instant::now()));
                                if left.is_zero() {
                                    Err(Error::TimedOut)
                                } else {
                                    Ok(left)
                                }
                            });
                            if left.is_err() {
                                self.stop_guard_scene(worker, owner);
                            }
                            left
                        };
                        let nested = CallContext { poll: &poll };
                        // Cooperative cancel/EOF follows scene -> resident order.
                        // A resident pipe/frame failure may retire itself inside
                        // Worker.call; that pre-existing bounded failure path
                        // keeps cleanup uncertainty instead of promising order.
                        let response = match self.read_rpc(validation.clone(), &nested) {
                            Ok(_) => json!({"ok":true}),
                            Err(error) => guard_error(error),
                        };
                        if query.respond(response).is_err() {
                            owner.invalidate();
                        }
                    }
                }
                match call.remaining() {
                    Ok(left) => {
                        let left = left.min(deadline.saturating_duration_since(Instant::now()));
                        if left.is_zero() {
                            if let Some((owner, _)) = guard {
                                owner.invalidate();
                            }
                            Err(StopReason::TimedOut)
                        } else {
                            Ok(left)
                        }
                    }
                    Err(error) => {
                        if let Some((owner, _)) = guard {
                            owner.invalidate();
                        }
                        *stop.lock().unwrap() = Some(error);
                        Err(StopReason::Cancelled)
                    }
                }
            })
            .map_err(|error| {
                let sent = error.request_may_have_been_sent;
                (transport_cause(error, stop.lock().unwrap().take()), sent)
            })?;
        let reply: Value =
            serde_json::from_slice(&bytes).map_err(|e| (retire_worker(worker, failed(e)), true))?;
        let reply = envelope_reply(&reply, request_id)
            .map_err(|error| (retire_worker(worker, error), true))?;
        if let Some(error) = native_cleanup_error(&reply) {
            self.cleanup_failure.lock().unwrap().get_or_insert(error);
        }
        if reply_error(&reply).is_none() {
            cursor_attempt.operation = None;
        }
        Ok(reply)
    }

    fn stop_guard_scene(&self, worker: &Worker, owner: &scroll_guard::Owner) {
        owner.invalidate();
        if let Err(error) = worker.stop() {
            // A cleanup error can coexist with a confirmed wait/reap. If the
            // OS could not terminate the owned scene at all, we still use the
            // existing bounded resident stop; neither cleanup is called proven.
            // Never prolong the original deadline or permit new input admission.
            let message = if worker.is_reaped() {
                format!("{error}")
            } else {
                format!("Scene process exit could not be confirmed before bounded resident cleanup: {error}")
            };
            let mut state = self.scene.state.lock().unwrap();
            state.failure.get_or_insert(Error::BackendFailed(message));
            state.closed = true;
        }
    }

    fn retire(&self, error: Error) -> Error {
        retire_worker(&self.worker, error)
    }
    fn read_rpc(&self, request: Value, call: &CallContext<'_>) -> Result<Value, Error> {
        let reply = self.rpc(request, call).map_err(|e| e.0)?;
        if reply.get("error").is_some_and(|e| !e.is_string()) {
            return Err(self.retire(Error::InvalidObservation));
        }
        if let Some(error) = reply_error(&reply) {
            Err(error)
        } else {
            Ok(reply)
        }
    }
    fn action(&self, request: Value, call: &CallContext<'_>) -> ActionOutcome {
        // A worker can restore focus during cancellation shutdown before the
        // finalizer can ask for its exact return result. Retain that uncertainty.
        let explicit_focus = request["operation"] == "focus";
        let previous_pending = explicit_focus && self.focus_pending.swap(true, Ordering::AcqRel);
        let reply = match self.rpc(request, call) {
            Ok(reply) => reply,
            Err((error, true)) => return ActionOutcome::unknown(error),
            Err((error, false)) => {
                if explicit_focus {
                    self.focus_pending
                        .store(previous_pending, Ordering::Release);
                }
                return ActionOutcome::rejected(error);
            }
        };
        let mut outcome = action_reply(&reply);
        if explicit_focus && focus_did_not_add_return_obligation(&outcome) {
            self.focus_pending
                .store(previous_pending, Ordering::Release);
        }
        if outcome.error == Some(Error::InvalidObservation) {
            outcome.error = Some(self.retire(Error::InvalidObservation));
        }
        outcome
    }

    fn validate_window(&self, target: &Value, call: &CallContext<'_>) -> Result<Value, Error> {
        let reply = self.read_rpc(json!({"operation":"validate","target":target}), call)?;
        worker::validate_reply(&reply, target)
            .map_err(|error| reply_error(&error).unwrap_or(Error::InvalidObservation))?;
        Ok(reply)
    }

    fn scene_rpc(&self, request: Value, call: &CallContext<'_>) -> Result<Value, (Error, bool)> {
        self.scene_rpc_guarded(request, call, None)
    }
    fn scene_rpc_guarded(
        &self,
        mut request: Value,
        call: &CallContext<'_>,
        validation: Option<Value>,
    ) -> Result<Value, (Error, bool)> {
        let original = request["target"].clone();
        // This first validation rejects stale refs before spawning a scene.
        self.validate_window(&original, call)
            .map_err(|error| (error, false))?;
        let guard = validation
            .as_ref()
            .map(|_| scroll_guard::Owner::new())
            .transpose()
            .map_err(|error| (failed(error), false))?;
        let scene = self
            .scene
            .start(|| {
                let exe = std::env::current_exe().map_err(failed)?;
                Worker::spawn(&exe, &["--dtw-scene-worker"]).map_err(transport_error)
            })
            .map_err(|error| (error, false))?;
        if let Some(guard) = &guard {
            self.scene
                .attach_guard(&scene.worker, guard.clone())
                .map_err(|error| (error, false))?;
            request["_scroll_guard"] = guard.endpoint();
        }
        let capture = request["operation"] == "capture";
        let result = (|| {
            // A scene registers only the selected surface in its own generation.
            // Revalidate the resident binding after acquisition to link both
            // identities without repeating optional app-wide discovery.
            let local = bind_scene_target(&original, |request| {
                self.rpc_on(&scene.worker, request, call)
                    .map_err(|(error, _)| error)
            })
            .map_err(|error| (error, false))?;
            let before = self
                .validate_window(&original, call)
                .map_err(|error| (error, false))?;
            request["target"] = local.clone();
            let reply = self.rpc_on_guarded(
                &scene.worker,
                request,
                call,
                guard.as_ref().zip(validation.as_ref()),
            )?;
            Ok((reply, local, before))
        })();
        // SCK can live until process exit, even after its reply. Resident
        // revalidation may need a fresh association proof child and must not
        // wait for the still-live Scene's global lease.
        let scene_worker = scene.worker.clone();
        let _ = scene.finish();
        // The owner retains cleanup failure on the independent diagnostic
        // channel. Never replace an original native/transport error with it.
        result.and_then(|(mut reply, local, before)| {
            if capture && reply.get("error").is_some_and(|error| !error.is_string()) {
                return Err((Error::InvalidObservation, false));
            }
            if capture
                && CaptureGeometryStage::from_reply(&reply).is_some()
                && (!scene_worker.is_reaped() || self.scene.cleanup_error().is_some())
            {
                // Preserve the original geometry failure and independent
                // cleanup diagnostic; an unretired SCK client cannot retry.
                return Err((reply_error(&reply).expect("geometry error"), false));
            }
            if capture && reply.get("error").is_none() {
                if reply["target"] != local {
                    return Err((Error::InvalidObservation, false));
                }
                if !scene_worker.is_reaped() {
                    return Err((
                        self.scene.cleanup_error().unwrap_or_else(|| {
                            Error::BackendFailed(
                                "Scene exit is unconfirmed; capture revalidation was not started"
                                    .into(),
                            )
                        }),
                        false,
                    ));
                }
                let after = self
                    .validate_window(&original, call)
                    .map_err(|error| (error, false))?;
                if let Some(error) = worker::capture_snapshot_error(
                    &before,
                    &after,
                    &reply["bounds"],
                    CaptureGeometryStage::Resident,
                ) {
                    if self.scene.cleanup_error().is_some() {
                        return Err((
                            reply_error(&error).expect("capture validation error"),
                            false,
                        ));
                    }
                    return Ok(error);
                }
                reply["target"] = original;
            }
            Ok(reply)
        })
    }
}

fn retire_worker(worker: &Worker, error: Error) -> Error {
    match worker.stop() {
        Ok(()) => error,
        Err(shutdown) => Error::BackendFailed(format!(
            "{error:?}; native worker shutdown failed: {shutdown}"
        )),
    }
}
fn focus_did_not_add_return_obligation(outcome: &ActionOutcome) -> bool {
    let borrowed = outcome
        .delivery
        .as_ref()
        .is_some_and(|value| value.foreground_borrowed);
    !borrowed
        && (outcome.dispatch == DispatchState::NotDispatched
            || outcome.delivery.as_ref().is_some_and(|value| {
                value.route == Some(InputRoute::Foreground)
                    && value.focus_return == Some(FocusReturn::Unchanged)
            }))
}

fn bind_scene_target(
    original: &Value,
    bind: impl FnOnce(Value) -> Result<Value, Error>,
) -> Result<Value, Error> {
    let original = parse_target(original)?;
    let inventory = bind(
        json!({"operation":"bind_window", "app":app_json(&original.app),
        "native_id":original.native_id}),
    )?;
    if inventory
        .get("error")
        .is_some_and(|error| !error.is_string())
    {
        return Err(Error::InvalidObservation);
    }
    if let Some(error) = reply_error(&inventory) {
        return Err(error);
    }
    select_scene_target(&inventory, &target_json(&original))
}

fn select_scene_target(inventory: &Value, original: &Value) -> Result<Value, Error> {
    let original = parse_target(original)?;
    let coverage = &inventory["coverage"];
    if coverage["scope"] != "selected_surface" || coverage["association_probe_failures"] != 0 {
        return Err(Error::InvalidObservation);
    }
    // This private binding report describes one selected surface, not app-wide
    // discovery. Reuse count and row validation without publishing its scope.
    let report = parse_surface_inventory(coverage, true)?;
    if report.candidate_count != 1 || report.supported_count != 1 || report.excluded_count != 0 {
        return Err(Error::InvalidObservation);
    }
    let windows = parse_window_rows(inventory, &original.app, report.supported_count)?;
    if windows.len() != 1 {
        return Err(Error::InvalidObservation);
    }
    windows
        .into_iter()
        .find(|window| window.target.native_id == original.native_id)
        .map(|window| target_json(&window.target))
        .ok_or(Error::TargetChanged)
}
impl Backend for NativeBackend {
    type Binding = NativeBinding;
    fn is_available(&self) -> bool {
        !self.worker.is_stopped()
    }
    fn finish_execution(&self) -> Result<Option<FocusReturn>, Error> {
        let until = Instant::now() + NORMAL_FOCUS_RETURN_BUDGET + FOCUS_RETURN_TRANSPORT_MARGIN;
        let request_id = unique_generation();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut request = focus_return_request(now, None);
        request["request_id"] = json!(request_id);
        let bytes = match self.worker.call(request.to_string().as_bytes(), || {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                Err(StopReason::TimedOut)
            } else {
                Ok(left)
            }
        }) {
            Ok(bytes) => bytes,
            Err(error) if error.kind == TransportErrorKind::Stopped => {
                return self.worker.cleanup_error().map_or_else(
                    || {
                        Ok(self
                            .focus_pending
                            .load(Ordering::Acquire)
                            .then_some(FocusReturn::Unconfirmed))
                    },
                    |error| Err(transport_error(error)),
                );
            }
            Err(error) => return Err(transport_error(error)),
        };
        let envelope = serde_json::from_slice(&bytes).map_err(failed)?;
        let reply = envelope_reply(&envelope, request_id)?;
        if let Some(error) = reply_error(&reply) {
            return Err(error);
        }
        self.focus_pending.store(false, Ordering::Release);
        // No outstanding borrow is not a new restoration claim.
        match reply["focus"].as_str() {
            Some("unchanged") => Ok(None),
            Some("restored") => Ok(Some(FocusReturn::Restored)),
            Some("application_restored") => Ok(Some(FocusReturn::ApplicationRestored)),
            Some("changed_externally") => Ok(Some(FocusReturn::ChangedExternally)),
            Some("previous_app_closed") => Ok(Some(FocusReturn::PreviousAppClosed)),
            Some("unconfirmed") => Ok(Some(FocusReturn::Unconfirmed)),
            _ => Err(Error::InvalidObservation),
        }
    }
    fn cleanup_error(&self) -> Option<Error> {
        self.cleanup_failure
            .lock()
            .unwrap()
            .clone()
            .or_else(|| self.scene.cleanup_error())
            .or_else(|| self.worker.cleanup_error().map(transport_error))
    }
    fn shutdown(&self) -> Result<(), Error> {
        let scene = self.scene.close();
        let native = self.worker.stop().map_err(transport_error);
        self.cursor
            .hide(self.cursor_operation.swap(0, Ordering::AcqRel));
        match (scene, native) {
            (Err(scene), Err(native)) => Err(failed(format!("{scene:?}; {native:?}"))),
            (Err(error), _) | (_, Err(error)) => Err(error),
            _ => self
                .cleanup_failure
                .lock()
                .unwrap()
                .clone()
                .map_or(Ok(()), Err),
        }
    }
    fn stamp(&self) -> ProviderStamp {
        self.stamp.clone()
    }
    fn supports(&self, capability: Capability) -> bool {
        Self::supports_capability(capability)
    }
    fn coverage(&self) -> &'static str {
        COVERAGE
    }
    fn conditions(&self) -> &'static [&'static str] {
        CONDITIONS
    }
    fn windows(&self, app: &AppInstance, call: &CallContext<'_>) -> Result<WindowInventory, Error> {
        let result = self.read_rpc(json!({"operation":"windows","app":app_json(app)}), call)?;
        parse_window_inventory(&result, app)
    }
    fn supported_window_wait_states(&self) -> &'static [WindowWaitState] {
        &WindowWaitState::ALL
    }
    fn probe_window_state(
        &self,
        target: &WindowTarget,
        state: WindowWaitState,
        call: &CallContext<'_>,
    ) -> Result<WindowStateProbe, Error> {
        let reply = self.read_rpc(json!({"operation":"probe_window_state", "target":target_json(target), "state":state.as_str()}), call)?;
        parse_window_state_probe(&reply, &target.app)
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<NativeBinding>, Error> {
        self.observe_page(target, &ObserveQuery::default(), call)
    }
    fn observe_page(
        &self,
        target: &WindowTarget,
        query: &ObserveQuery,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<NativeBinding>, Error> {
        self.observe_scoped(target, query, ObserveScope::Window, None, call)
    }
    fn supports_observe_scope(&self, _scope: ObserveScopeKind) -> bool {
        true
    }
    fn observe_scoped(
        &self,
        target: &WindowTarget,
        query: &ObserveQuery,
        scope: ObserveScope,
        source: Option<&NativeBinding>,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<NativeBinding>, Error> {
        let native_scope = match scope {
            ObserveScope::Window => json!({"kind":"window"}),
            ObserveScope::Menu => json!({"kind":"menu"}),
            ObserveScope::Subtree(element) => {
                json!({"kind":"subtree","binding":source.and_then(|b|b.semantic.as_ref()).ok_or(Error::InvalidObservation)?,"element":element.0})
            }
            ObserveScope::Region(region) => {
                let (bounds, width, height) = source
                    .and_then(|b| b.pixels.as_ref())
                    .ok_or(Error::InvalidObservation)?;
                let (x, y) = bound_pixel_point(
                    bounds,
                    *width,
                    *height,
                    PixelPoint {
                        x: region.x,
                        y: region.y,
                    },
                )?;
                let w = bounds[2].as_f64().ok_or(Error::InvalidObservation)? * region.width
                    / *width as f64;
                let h = bounds[3].as_f64().ok_or(Error::InvalidObservation)? * region.height
                    / *height as f64;
                json!({"kind":"region","bounds":bounds,"rect":[x,y,w,h]})
            }
        };
        let reply = self.read_rpc(
            json!({"operation":"observe","target":target_json(target),
                "query":{"role":query.role,"text":query.text,"limit":query.limit,"scope":native_scope}}),
            call,
        )?;
        // Construct the guard before parsing the rest so invalid replies still release it.
        let binding = NativeBinding {
            worker: self.worker.clone(),
            semantic: Some(string(&reply, "binding")?.into()),
            pixels: None,
        };
        let page = parse_discovery_page(&reply)?;
        Ok(BackendObservation {
            target: page.target,
            elements: page.elements,
            image: None,
            discovery: Some(page.discovery),
            binding,
        })
    }
    fn read_text(
        &self,
        target: &WindowTarget,
        binding: &NativeBinding,
        request: TextRequest,
        call: &CallContext<'_>,
    ) -> Result<BackendTextPage, Error> {
        let reply = self.read_rpc(json!({"operation":"read_text","target":target_json(target),
            "binding":binding.semantic.as_ref().ok_or(Error::InvalidObservation)?,"element":request.element.0,
            "offset":request.offset,"limit":request.limit}), call)?;
        if reply["unit"] != "utf16" {
            return Err(Error::InvalidObservation);
        }
        Ok(BackendTextPage {
            target: parse_target(&reply["target"])?,
            page: TextPage {
                text: string(&reply, "text")?.into(),
                offset: u32::try_from(number(&reply, "offset")?).map_err(failed)?,
                total_length: u32::try_from(number(&reply, "total_length")?).map_err(failed)?,
                next_offset: if reply["next_offset"].is_null() {
                    None
                } else {
                    Some(u32::try_from(number(&reply, "next_offset")?).map_err(failed)?)
                },
            },
        })
    }
    fn continue_observation(
        &self,
        target: &WindowTarget,
        binding: &NativeBinding,
        call: &CallContext<'_>,
    ) -> Result<BackendObservationPage, Error> {
        let token = binding.semantic.as_ref().ok_or(Error::InvalidObservation)?;
        let reply = self.read_rpc(
            json!({"operation":"observe_next","target":target_json(target),"binding":token}),
            call,
        )?;
        if reply["binding"].as_str() != Some(token.as_str()) {
            return Err(Error::InvalidObservation);
        }
        parse_discovery_page(&reply)
    }
    fn press(
        &self,
        target: &WindowTarget,
        binding: &NativeBinding,
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        let Some(token) = &binding.semantic else {
            return ActionOutcome::rejected(Error::InvalidObservation);
        };
        self.action(json!({"operation":"press","target":target_json(target),"binding":token,"element":element.0}),call)
    }
    fn capture(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<NativeBinding>, Error> {
        let reply =
            fresh_capture_reply(target, call, |request, call| self.scene_rpc(request, call))?;
        let width = u32::try_from(number(&reply, "width")?).map_err(failed)?;
        let height = u32::try_from(number(&reply, "height")?).map_err(failed)?;
        let png = base64::engine::general_purpose::STANDARD
            .decode(string(&reply, "png")?)
            .map_err(failed)?;
        let bounds = &reply["bounds"];
        let coords = bounds
            .as_array()
            .filter(|a| a.len() == 4)
            .ok_or(Error::InvalidObservation)?;
        if coords
            .iter()
            .any(|n| n.as_f64().is_none_or(|f| !f.is_finite()))
            || coords[2].as_f64().unwrap() <= 0.0
            || coords[3].as_f64().unwrap() <= 0.0
        {
            return Err(Error::InvalidObservation);
        }
        Ok(BackendObservation {
            target: parse_target(&reply["target"])?,
            elements: vec![],
            image: Some(ImageFrame { width, height, png }),
            discovery: None,
            binding: NativeBinding {
                worker: self.worker.clone(),
                semantic: None,
                pixels: Some((bounds.clone(), width, height)),
            },
        })
    }
    fn click(
        &self,
        target: &WindowTarget,
        binding: &NativeBinding,
        point: PixelPoint,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.click_with_options(target, binding, point, ClickOptions::default(), call)
    }
    fn click_with_options(
        &self,
        target: &WindowTarget,
        binding: &NativeBinding,
        point: PixelPoint,
        options: ClickOptions,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        let Some((bounds, width, height)) = &binding.pixels else {
            return ActionOutcome::rejected(Error::InvalidObservation);
        };
        let (screen_x, screen_y) = match bound_pixel_point(bounds, *width, *height, point) {
            Ok(point) => point,
            Err(error) => return ActionOutcome::rejected(error),
        };
        let scene = self.scene_rpc(json!({"operation":"click","target":target_json(target),"bounds":bounds,"screen_x":screen_x,"screen_y":screen_y,"button":match options.button {MouseButton::Left=>"left",MouseButton::Right=>"right"},"count":options.count}),call);
        match scene {
            Ok(reply) => action_reply(&reply),
            Err((error, true)) => ActionOutcome::unknown(error),
            Err((error, false)) => ActionOutcome::rejected(error),
        }
    }
    fn supports_scroll_anchor(&self, kind: ObservationKind) -> bool {
        matches!(kind, ObservationKind::Pixels | ObservationKind::Semantic)
    }
    fn scroll_at(
        &self,
        target: &WindowTarget,
        binding: &NativeBinding,
        scroll: ScrollRequest,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        let original = target_json(target);
        let mut request = json!({"operation":"scroll","target":original,
            "delta_x":scroll.delta_x,"delta_y":scroll.delta_y});
        let validation = match scroll.anchor {
            ScrollAnchor::Pixels(point) => {
                let Some((bounds, width, height)) = &binding.pixels else {
                    return ActionOutcome::rejected(Error::InvalidObservation);
                };
                let (x, y) = match bound_pixel_point(bounds, *width, *height, point) {
                    Ok(point) => point,
                    Err(error) => return ActionOutcome::rejected(error),
                };
                request["bounds"] = bounds.clone();
                request["screen_x"] = json!(x);
                request["screen_y"] = json!(y);
                None
            }
            ScrollAnchor::Element(element) => {
                let Some(token) = &binding.semantic else {
                    return ActionOutcome::rejected(Error::InvalidObservation);
                };
                let mut proof = json!({"operation":"resolve_scroll_anchor","target":original,
                    "binding":token,"element":element.0});
                let resolved = match self.read_rpc(proof.clone(), call) {
                    Ok(value) => value,
                    Err(error) => return ActionOutcome::rejected(error),
                };
                if worker::validate_reply(&resolved, &original).is_err()
                    || !["screen_x", "screen_y"]
                        .iter()
                        .all(|key| resolved[key].as_f64().is_some_and(f64::is_finite))
                    || resolved["element_bounds"].as_array().is_none_or(|values| {
                        values.len() != 4
                            || values
                                .iter()
                                .any(|value| value.as_f64().is_none_or(|value| !value.is_finite()))
                    })
                {
                    return ActionOutcome::rejected(self.retire(Error::InvalidObservation));
                }
                proof["operation"] = json!("validate_scroll_anchor");
                for key in ["bounds", "screen_x", "screen_y", "element_bounds"] {
                    proof[key] = resolved[key].clone();
                    request[key] = resolved[key].clone();
                }
                Some(proof)
            }
        };
        match self.scene_rpc_guarded(request, call, validation) {
            Ok(reply) => action_reply(&reply),
            Err((error, true)) => ActionOutcome::unknown(error),
            Err((error, false)) => ActionOutcome::rejected(error),
        }
    }
    fn input(
        &self,
        target: &WindowTarget,
        input: &WindowInput,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        let mut request = match input {
            WindowInput::Focus => json!({"operation":"focus"}),
            WindowInput::TypeText { text } => json!({"operation":"type_text","text":text}),
            WindowInput::Keypress { keys } => json!({"operation":"keypress","keys":keys}),
            WindowInput::Scroll { delta_x, delta_y } => {
                json!({"operation":"scroll","delta_x":delta_x,"delta_y":delta_y})
            }
        };
        request["target"] = target_json(target);
        if matches!(input, WindowInput::Scroll { .. }) {
            return match self.scene_rpc(request, call) {
                Ok(reply) => action_reply(&reply),
                Err((error, true)) => ActionOutcome::unknown(error),
                Err((error, false)) => ActionOutcome::rejected(error),
            };
        }
        self.action(request, call)
    }
    fn set_value(
        &self,
        target: &WindowTarget,
        binding: &NativeBinding,
        element: ElementId,
        value: &str,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        let Some(token) = &binding.semantic else {
            return ActionOutcome::rejected(Error::InvalidObservation);
        };
        self.action(
            json!({"operation":"set_value","target":target_json(target),"binding":token,
                           "element":element.0,"value":value}),
            call,
        )
    }
}
fn guard_error(error: Error) -> Value {
    match error {
        Error::NativeFailure { code, message } => json!({"error":code,"message":message}),
        Error::TimedOut => {
            json!({"error":"deadline_exceeded","message":"Scroll anchor validation deadline elapsed"})
        }
        Error::Cancelled | Error::Closed => {
            json!({"error":"native_stopping","message":"Scroll anchor validation cancelled"})
        }
        other => json!({"error":"scroll_anchor_unavailable","message":format!("{other:?}")}),
    }
}
fn bound_pixel_point(
    bounds: &Value,
    width: u32,
    height: u32,
    point: PixelPoint,
) -> Result<(f64, f64), Error> {
    let values = bounds
        .as_array()
        .filter(|values| values.len() == 4)
        .ok_or(Error::InvalidObservation)?;
    let coords = values
        .iter()
        .map(|value| value.as_f64().filter(|value| value.is_finite()))
        .collect::<Option<Vec<_>>>()
        .ok_or(Error::InvalidObservation)?;
    if width == 0 || height == 0 || coords[2] <= 0.0 || coords[3] <= 0.0 {
        return Err(Error::InvalidObservation);
    }
    if !point.x.is_finite()
        || !point.y.is_finite()
        || point.x < 0.0
        || point.y < 0.0
        || point.x >= width as f64
        || point.y >= height as f64
    {
        return Err(Error::InvalidPoint);
    }
    let x = coords[0] + point.x / width as f64 * coords[2];
    let y = coords[1] + point.y / height as f64 * coords[3];
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::InvalidPoint);
    }
    Ok((x, y))
}
fn native_cleanup_error(reply: &Value) -> Option<Error> {
    let cleanup = reply.get("cleanup_error")?;
    Some(Error::NativeFailure {
        code: cleanup["code"]
            .as_str()
            .or_else(|| cleanup["error"].as_str())
            .unwrap_or("cleanup_failed")
            .into(),
        message: cleanup["message"]
            .as_str()
            .unwrap_or("Native resource cleanup could not be confirmed")
            .into(),
    })
}
fn envelope_reply(reply: &Value, request_id: u64) -> Result<Value, Error> {
    if reply["request_id"].as_u64() != Some(request_id) || !reply["result"].is_object() {
        Err(Error::InvalidObservation)
    } else {
        Ok(reply["result"].clone())
    }
}
fn parse_delivery(value: Option<&Value>) -> Result<Option<InputDelivery>, ()> {
    let Some(value) = value else { return Ok(None) };
    let object = value.as_object().ok_or(())?;
    let route = match object.get("route").and_then(Value::as_str) {
        Some("semantic") => Some(InputRoute::Semantic),
        Some("background") => Some(InputRoute::Background),
        Some("foreground") => Some(InputRoute::Foreground),
        None if value["route"].is_null() => None,
        _ => return Err(()),
    };
    let reason = match object.get("reason") {
        Some(Value::String(reason)) if reason.len() <= 2048 => Some(reason.clone()),
        None | Some(Value::Null) => None,
        _ => return Err(()),
    };
    let foreground_borrowed = value["foreground_borrowed"].as_bool().ok_or(())?;
    let focus_return = match object.get("focus_return").and_then(Value::as_str) {
        Some("pending") => Some(FocusReturn::Pending),
        Some("restored") => Some(FocusReturn::Restored),
        Some("application_restored") => Some(FocusReturn::ApplicationRestored),
        Some("changed_externally") => Some(FocusReturn::ChangedExternally),
        Some("previous_app_closed") => Some(FocusReturn::PreviousAppClosed),
        Some("unchanged") => Some(FocusReturn::Unchanged),
        Some("unconfirmed") => Some(FocusReturn::Unconfirmed),
        None if value["focus_return"].is_null() => None,
        _ => return Err(()),
    };
    if (foreground_borrowed || focus_return.is_some()) && route != Some(InputRoute::Foreground) {
        return Err(());
    }
    Ok(Some(InputDelivery {
        route,
        reason,
        foreground_borrowed,
        focus_return,
    }))
}

fn action_reply(reply: &Value) -> ActionOutcome {
    let dispatch = match reply["dispatch"].as_str() {
        Some("not_dispatched") => DispatchState::NotDispatched,
        Some("dispatched") => DispatchState::Dispatched,
        Some("unknown") => DispatchState::Unknown,
        _ => return ActionOutcome::unknown(Error::InvalidObservation),
    };
    let error = reply_error(reply);
    if reply["evidence"] != "unverified"
        || reply.get("error").is_some_and(|e| !e.is_string())
        || (dispatch == DispatchState::NotDispatched && error.is_none())
        || (dispatch == DispatchState::Dispatched && error.is_some())
    {
        return ActionOutcome::unknown(Error::InvalidObservation);
    }
    let delivery = match parse_delivery(reply.get("delivery")) {
        Ok(delivery) => delivery,
        Err(()) => return ActionOutcome::unknown(Error::InvalidObservation),
    };
    ActionOutcome {
        delivery,
        dispatch,
        evidence: EffectEvidence::Unverified,
        error,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn capture_target() -> WindowTarget {
        WindowTarget {
            app: AppInstance {
                identity: AppIdentity("capture.app".into()),
                pid: 21,
                generation: 30,
            },
            native_id: 42,
            generation: 55,
        }
    }
    fn capture_validation(target: &WindowTarget, bounds: Value) -> Value {
        json!({"target":target_json(target),"surface_owner":{"pid":21,"generation":30},"bounds":bounds})
    }

    #[test]
    fn fresh_capture_rechecks_same_target_and_live_budget_then_only_returns_new_image() {
        for stage in [CaptureGeometryStage::Scene, CaptureGeometryStage::Resident] {
            let target = capture_target();
            let budget = AtomicU64::new(2000);
            let poll = || Ok(Duration::from_millis(budget.load(Ordering::SeqCst)));
            let call = CallContext { poll: &poll };
            let mut attempts = 0;
            let before = capture_validation(&target, json!([1, 2, 300, 400]));
            let after = capture_validation(&target, json!([2, 3, 300, 400]));
            let result = fresh_capture_reply(&target,&call,|request,context| {
                attempts += 1;
                assert_eq!(request,json!({"operation":"capture","target":target_json(&target)}));
                assert!(std::ptr::eq(context,&call),"capture replaced the caller's live deadline");
                if attempts == 1 {
                    assert_eq!(context.remaining(),Ok(Duration::from_millis(2000)));
                    budget.store(900,Ordering::SeqCst);
                    let mut moved = worker::capture_snapshot_error(&before,&after,&before["bounds"],stage).unwrap();
                    moved["png"] = json!("obsolete-first-frame");
                    Ok(moved)
                } else {
                    assert_eq!(context.remaining(),Ok(Duration::from_millis(900)));
                    assert!(worker::capture_snapshot_error(&after,&after,&after["bounds"],stage).is_none());
                    Ok(json!({"target":target_json(&target),"bounds":after["bounds"],"png":"fresh-second-frame"}))
                }
            }).unwrap();
            assert_eq!(attempts, 2);
            assert_eq!(result["png"], "fresh-second-frame");
            assert_eq!(result["bounds"], after["bounds"]);
        }
    }

    #[test]
    fn fresh_capture_owner_change_is_not_a_geometry_retry_even_when_bounds_also_change() {
        let target = capture_target();
        let poll = || Ok(Duration::from_secs(1));
        let call = CallContext { poll: &poll };
        let before = capture_validation(&target, json!([1, 2, 300, 400]));
        for field in ["pid", "generation"] {
            let mut after = capture_validation(&target, json!([2, 3, 300, 400]));
            after["surface_owner"][field] = json!(999);
            let mut attempts = 0;
            let result = fresh_capture_reply(&target, &call, |_, _| {
                attempts += 1;
                Ok(worker::capture_snapshot_error(
                    &before,
                    &after,
                    &before["bounds"],
                    CaptureGeometryStage::Scene,
                )
                .unwrap())
            });
            assert_eq!(result, Err(Error::TargetChanged));
            assert_eq!(attempts, 1);
        }
    }

    #[test]
    fn fresh_capture_never_retries_stale_timeout_permission_or_unclassified_reply() {
        let target = capture_target();
        let poll = || Ok(Duration::from_secs(1));
        let call = CallContext { poll: &poll };
        for reply in [
            json!({"error":"stale_observation","message":"Old observation"}),
            json!({"error":"deadline_exceeded","message":"Original deadline"}),
            json!({"error":"permission_denied","message":"Screen recording permission missing"}),
            json!({"error":"capture_geometry_changed","message":"Untyped geometry text"}),
            json!({"error":"capture_geometry_changed","message":"Unknown stage","details":{"capture_stage":"other"}}),
            json!({"error":"capture_geometry_changed","message":"Unknown effect","details":{"capture_stage":"scene"},"dispatch":"unknown"}),
        ] {
            let expected = reply_error(&reply).unwrap();
            let mut attempts = 0;
            let result = fresh_capture_reply(&target, &call, |_, _| {
                attempts += 1;
                Ok(reply.clone())
            });
            assert_eq!(result, Err(expected));
            assert_eq!(attempts, 1);
        }
        for (error, sent) in [
            (Error::StaleObservation, false),
            (Error::TimedOut, true),
            (Error::Cancelled, true),
            (Error::TargetChanged, false),
            (
                reply_error(&CaptureGeometryStage::Scene.reply()).unwrap(),
                true,
            ),
        ] {
            let mut attempts = 0;
            let result = fresh_capture_reply(&target, &call, |_, _| {
                attempts += 1;
                Err((error.clone(), sent))
            });
            assert_eq!(result, Err(error));
            assert_eq!(
                attempts, 1,
                "transport uncertainty must not become another read"
            );
        }
    }

    #[test]
    fn fresh_capture_second_failure_retains_its_stage_and_never_publishes_old_frame() {
        let target = capture_target();
        let poll = || Ok(Duration::from_secs(1));
        let call = CallContext { poll: &poll };
        for second in [
            CaptureGeometryStage::Resident.reply(),
            json!({"error":"target_changed","message":"Owner replaced before second frame"}),
        ] {
            let mut attempts = 0;
            let result = fresh_capture_reply(&target, &call, |_, _| {
                attempts += 1;
                if attempts == 1 {
                    let mut first = CaptureGeometryStage::Scene.reply();
                    first["png"] = json!("obsolete-first-frame");
                    Ok(first)
                } else {
                    Ok(second.clone())
                }
            });
            assert_eq!(result, Err(reply_error(&second).unwrap()));
            assert_eq!(attempts, 2);
        }
    }

    #[test]
    fn fresh_capture_checks_cancellation_and_original_deadline_before_second_read() {
        let target = capture_target();
        for interruption in [Error::Cancelled, Error::TimedOut] {
            let interrupted = AtomicBool::new(false);
            let poll = || {
                if interrupted.load(Ordering::SeqCst) {
                    Err(interruption.clone())
                } else {
                    Ok(Duration::from_secs(1))
                }
            };
            let call = CallContext { poll: &poll };
            let mut attempts = 0;
            let result = fresh_capture_reply(&target, &call, |_, _| {
                attempts += 1;
                interrupted.store(true, Ordering::SeqCst);
                Ok(CaptureGeometryStage::Scene.reply())
            });
            assert_eq!(result, Err(interruption));
            assert_eq!(attempts, 1);
        }
    }

    #[test]
    fn window_wait_provider_parses_only_explicit_scoped_proofs() {
        let app = AppInstance {
            identity: AppIdentity("wait.app".into()),
            pid: 42,
            generation: 7,
        };
        let target = WindowTarget {
            app: app.clone(),
            native_id: 2,
            generation: 3,
        };
        let valid = json!({"status":"matched", "kind":"sheet", "relation":"child_modal",
            "windows":[{"target":target_json(&target),"title":"","onscreen":true}]});
        assert!(matches!(
            parse_window_state_probe(&valid, &app),
            Ok(WindowStateProbe::Window(_))
        ));
        assert_eq!(
            parse_window_state_probe(&json!({"status":"closed"}), &app),
            Ok(WindowStateProbe::Closed)
        );
        assert!(
            parse_window_state_probe(&json!({"status":"pending","reason":"unknown"}), &app)
                .is_err()
        );
        for invalid in [
            json!({"status":"matched","kind":"sheet","relation":"child_modal","windows":[]}),
            json!({"status":"missing"}),
            json!({"status":"matched","kind":"guessed_dialog","relation":"child_modal","windows":valid["windows"]}),
        ] {
            assert!(parse_window_state_probe(&invalid, &app).is_err());
        }
        let mut wrong = valid;
        wrong["windows"][0]["target"]["app"]["pid"] = json!(43);
        assert_eq!(parse_window_state_probe(&wrong, &app), Err(Error::WrongApp));
        assert_eq!(
            NativeBackend::description()["supported_window_wait_states"],
            json!(["dialog", "unblocked", "closed"])
        );
    }

    #[test]
    fn normal_focus_return_budget_respects_the_original_call_deadline() {
        assert_eq!(focus_return_deadline_ms(1_000, None), 3_000);
        assert_eq!(focus_return_deadline_ms(1_000, Some(20_000)), 3_000);
        assert_eq!(focus_return_deadline_ms(1_000, Some(1_175)), 1_175);
        assert_eq!(focus_return_deadline_ms(1_000, Some(1_000)), 1_000);
        assert_eq!(focus_return_deadline_ms(1_000, Some(900)), 900);
        assert_eq!(focus_return_deadline_ms(u64::MAX - 50, None), u64::MAX);
        let short = focus_return_request(1_000, Some(1_175));
        assert_eq!(short["deadline_unix_ms"], 1_175);
        assert_eq!(short["ownership_cap_unix_ms"], 3_000);
        assert_eq!(short["caller_deadline_unix_ms"], 1_175);
        let long = focus_return_request(1_000, Some(20_000));
        assert_eq!(long["deadline_unix_ms"], 3_000);
        assert_eq!(long["ownership_cap_unix_ms"], 3_000);
        assert_eq!(long["caller_deadline_unix_ms"], 20_000);
        assert!(focus_return_request(1_000, None)
            .get("caller_deadline_unix_ms")
            .is_none());
        assert_eq!(
            NORMAL_FOCUS_RETURN_BUDGET + FOCUS_RETURN_TRANSPORT_MARGIN,
            Duration::from_millis(2_100)
        );
    }

    #[test]
    fn scroll_pixel_binding_uses_image_scale_and_negative_screen_origin() {
        let bounds = json!([-1200.0, -100.0, 800.0, 600.0]);
        assert_eq!(
            bound_pixel_point(&bounds, 1600, 1200, PixelPoint { x: 200.0, y: 100.0 }),
            Ok((-1100.0, -50.0))
        );
        assert_eq!(
            bound_pixel_point(&bounds, 1600, 1200, PixelPoint { x: 1600.0, y: 0.0 }),
            Err(Error::InvalidPoint)
        );
        assert_eq!(
            bound_pixel_point(&bounds, 0, 1200, PixelPoint { x: 0.0, y: 0.0 }),
            Err(Error::InvalidObservation)
        );
        assert_eq!(
            bound_pixel_point(
                &bounds,
                1600,
                1200,
                PixelPoint {
                    x: f64::NAN,
                    y: 0.0
                }
            ),
            Err(Error::InvalidPoint)
        );
    }

    #[test]
    fn scroll_guard_cancel_releases_scene_before_old_explicit_focus_owner() {
        // The scene holds the same kind of exclusion an old resident focus
        // cleanup needs. Nested polling must stop that scene first, even when
        // its exit reports cleanup failure after being conclusively reaped.
        for cleanup_fails in [false, true] {
            let path = std::env::temp_dir().join(format!(
                "dtw-scroll-order-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir(&path).unwrap();
            let prelude = format!(
                r#"
import sys,json,signal,time,pathlib,fcntl,socket
root=pathlib.Path({})
lock=open(root/'coordination','w')
def note(value):
 with open(root/'events','a') as log: log.write(value+'\n')
"#,
                json!(path.to_str().unwrap())
            );
            let scene_code = format!(
                r#"{prelude}
fcntl.flock(lock,fcntl.LOCK_SH)
def stop(signum,frame):
 fcntl.flock(lock,fcntl.LOCK_UN)
 note('scene-released')
 sys.exit({})
signal.signal(signal.SIGTERM,stop)
for line in sys.stdin:
 request=json.loads(line); guard=request['_scroll_guard']
 stream=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);stream.connect(guard['path'])
 stream.sendall((json.dumps({{'command':'check','token':guard['token']}})+'\n').encode())
 stream.recv(8192)
 while True:time.sleep(.01)
"#,
                if cleanup_fails { 74 } else { 0 }
            );
            let resident_code = format!(
                r#"{prelude}
def stop(signum,frame):
 note('resident-stopping')
 fcntl.flock(lock,fcntl.LOCK_EX)
 note('resident-restored')
 sys.exit({})
signal.signal(signal.SIGTERM,stop)
for line in sys.stdin:
 note('resident-reading')
 while True:time.sleep(.01)
"#,
                if cleanup_fails { 74 } else { 0 }
            );
            let backend = NativeBackend {
                worker: Arc::new(scene_fixture(&resident_code).unwrap()),
                scene: SceneOwner::default(),
                stamp: ProviderStamp {
                    id: "guard-order".into(),
                    generation: 1,
                },
                cleanup_failure: Mutex::new(None),
                input_policy: InputPolicy::Auto,
                focus_pending: AtomicBool::new(true),
                cursor: Arc::new(CursorSession::default()),
                cursor_operation: AtomicU64::new(0),
            };
            let scene = backend.scene.start(|| scene_fixture(&scene_code)).unwrap();
            let owner = scroll_guard::Owner::new().unwrap();
            backend
                .scene
                .attach_guard(&scene.worker, owner.clone())
                .unwrap();
            let until = Instant::now() + Duration::from_secs(3);
            let poll = || {
                if std::fs::read_to_string(path.join("events"))
                    .unwrap_or_default()
                    .contains("resident-reading")
                {
                    Err(Error::Cancelled) // Includes session cancellation / input EOF.
                } else {
                    let left = until.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        Err(Error::TimedOut)
                    } else {
                        Ok(left)
                    }
                }
            };
            let outcome = backend.rpc_on_guarded(
                &scene.worker,
                json!({"operation":"guard_fixture","_scroll_guard":owner.endpoint()}),
                &CallContext { poll: &poll },
                Some((&owner, &json!({"operation":"validate_scroll_anchor"}))),
            );
            assert!(
                matches!(outcome, Err((Error::Cancelled, true))),
                "{outcome:?}"
            );
            assert!(scene.worker.is_reaped());
            assert!(backend.worker.is_reaped());
            assert_eq!(scene.worker.cleanup_error().is_some(), cleanup_fails);
            assert_eq!(backend.worker.cleanup_error().is_some(), cleanup_fails);
            let events = std::fs::read_to_string(path.join("events")).unwrap();
            assert_eq!(
                events.lines().collect::<Vec<_>>(),
                [
                    "resident-reading",
                    "scene-released",
                    "resident-stopping",
                    "resident-restored"
                ]
            );
            assert_eq!(scene.finish().is_err(), cleanup_fails);
            if cleanup_fails {
                assert!(backend.scene.check().is_err());
            }
            drop(backend);
            std::fs::remove_dir_all(path).unwrap();
        }
    }

    #[test]
    fn stopped_focus_owner_keeps_unconfirmed_terminal_evidence() {
        let backend = NativeBackend {
            worker: Arc::new(scene_fixture("import sys\nfor line in sys.stdin: pass\n").unwrap()),
            scene: SceneOwner::default(),
            stamp: ProviderStamp {
                id: "focus-finalizer".into(),
                generation: 1,
            },
            cleanup_failure: Mutex::new(None),
            input_policy: InputPolicy::Auto,
            cursor: Arc::new(CursorSession::default()),
            cursor_operation: AtomicU64::new(0),
            focus_pending: AtomicBool::new(true),
        };
        backend.worker.stop().unwrap();
        assert_eq!(
            backend.finish_execution(),
            Ok(Some(FocusReturn::Unconfirmed))
        );
        backend.focus_pending.store(false, Ordering::Release);
        assert_eq!(backend.finish_execution(), Ok(None));
    }

    #[test]
    fn unconfirmed_focus_retirement_preserves_original_error_and_read_access() {
        let backend = NativeBackend {
            worker: Arc::new(scene_fixture(r#"import sys,json
for line in sys.stdin:
 r=json.loads(line)
 if r['operation']=='release_focus':
  assert r['ownership_cap_unix_ms']==r['deadline_unix_ms']
  result={'focus':'unconfirmed'}
 elif r['operation']=='keypress':
  result={'dispatch':'unknown','evidence':'unverified','error':'ax_unavailable','message':'AXWindows busy','delivery':{'route':'foreground','foreground_borrowed':True,'focus_return':'pending'}}
 else:
  result={'fresh_read':True}
 print(json.dumps({'request_id':r['request_id'],'result':result}),flush=True)
"#).unwrap()),
            scene: SceneOwner::default(),
            stamp: ProviderStamp { id: "focus-retirement".into(), generation: 1 },
            cleanup_failure: Mutex::new(None),
            input_policy: InputPolicy::Auto,
            cursor: Arc::new(CursorSession::default()),
            cursor_operation: AtomicU64::new(0),
            focus_pending: AtomicBool::new(true),
        };
        let poll = || Ok(Duration::from_secs(2));
        let call = CallContext { poll: &poll };
        let original = backend.action(json!({"operation":"keypress"}), &call);
        assert_eq!(original.dispatch, DispatchState::Unknown);
        assert_eq!(
            original.error,
            Some(Error::NativeFailure {
                code: "ax_unavailable".into(),
                message: "AXWindows busy".into()
            })
        );
        assert_eq!(
            backend.finish_execution(),
            Ok(Some(FocusReturn::Unconfirmed))
        );
        assert!(!backend.focus_pending.load(Ordering::Acquire));
        assert_eq!(backend.cleanup_error(), None);
        assert!(backend.is_available());
        assert_eq!(
            backend
                .read_rpc(json!({"operation":"read"}), &call)
                .unwrap()["fresh_read"],
            true
        );
        assert_eq!(original.dispatch, DispatchState::Unknown);
        backend.shutdown().unwrap();
    }

    #[test]
    fn action_keeps_native_route_and_return_without_claiming_effect() {
        for (route, borrowed, returned) in [
            ("semantic", false, None),
            ("background", false, None),
            ("foreground", true, Some("restored")),
            ("foreground", true, Some("changed_externally")),
            ("foreground", true, Some("pending")),
            ("foreground", false, Some("unchanged")),
        ] {
            let reply = json!({"dispatch":"dispatched","evidence":"unverified",
                "delivery":{"route":route,"reason":"test_reason","foreground_borrowed":borrowed,"focus_return":returned}});
            let result = action_reply(&reply);
            assert_eq!(result.dispatch, DispatchState::Dispatched);
            assert_eq!(result.evidence, EffectEvidence::Unverified);
            let delivery = result.delivery.unwrap();
            assert_eq!(delivery.route.unwrap().as_str(), route);
            assert_eq!(delivery.foreground_borrowed, borrowed);
            assert_eq!(delivery.focus_return.map(|value| value.as_str()), returned);
        }
        let rejected = action_reply(&json!({"dispatch":"not_dispatched","evidence":"unverified",
            "error":"foreground_required","message":"strict service", "delivery":{
                "route":null,"reason":"explicit_focus","foreground_borrowed":false}}));
        assert_eq!(rejected.dispatch, DispatchState::NotDispatched);
        assert_eq!(rejected.delivery.unwrap().route, None);
    }

    #[test]
    fn unchanged_focus_does_not_add_pending_return_but_unknown_focus_does() {
        for borrowed in [false, true] {
            let outcome = action_reply(&json!({"dispatch":"dispatched","evidence":"unverified",
                "delivery":{"route":"foreground","foreground_borrowed":borrowed,
                "focus_return":if borrowed {"pending"} else {"unchanged"}}}));
            assert_eq!(focus_did_not_add_return_obligation(&outcome), !borrowed);
        }
        assert!(focus_did_not_add_return_obligation(
            &ActionOutcome::rejected(Error::TargetChanged)
        ));
        assert!(!focus_did_not_add_return_obligation(
            &ActionOutcome::unknown(Error::TargetChanged)
        ));
        let malformed = action_reply(&json!({"dispatch":"dispatched","evidence":"unverified",
            "delivery":{"route":"invalid","foreground_borrowed":false,"focus_return":"unchanged"}}));
        assert!(!focus_did_not_add_return_obligation(&malformed));
    }

    #[test]
    fn malformed_route_metadata_never_fabricates_delivery() {
        for delivery in [
            json!({"route":"guessed","foreground_borrowed":false}),
            json!({"route":"background","foreground_borrowed":true}),
            json!({"route":"semantic","foreground_borrowed":false,"focus_return":"restored"}),
            json!({"route":"foreground","foreground_borrowed":true,"focus_return":"success"}),
        ] {
            let result = action_reply(
                &json!({"dispatch":"dispatched","evidence":"unverified","delivery":delivery}),
            );
            assert_eq!(result, ActionOutcome::unknown(Error::InvalidObservation));
        }
    }

    #[test]
    fn cleanup_failure_survives_an_independent_primary_action_error() {
        let backend = NativeBackend {
            worker: Arc::new(scene_fixture("import sys,json\nfor line in sys.stdin:\n r=json.loads(line);print(json.dumps({'request_id':r['request_id'],'result':{'dispatch':'unknown','evidence':'unverified','error':'input_failed','message':'original action error','cleanup_error':{'error':'focus_restore_failed','message':'restore failed'}}}),flush=True)").unwrap()),
            scene: SceneOwner::default(),
            cleanup_failure: Mutex::new(None),
            cursor: Arc::new(CursorSession::default()),
            cursor_operation: AtomicU64::new(0),
            focus_pending: AtomicBool::new(false),
            input_policy: InputPolicy::Auto,
            stamp: ProviderStamp { id:"cleanup-test".into(),generation:1 },
        };
        let poll = || Ok(Duration::from_secs(2));
        let outcome = backend.action(
            json!({"operation":"keypress"}),
            &CallContext { poll: &poll },
        );
        assert_eq!(
            outcome.error,
            Some(Error::NativeFailure {
                code: "input_failed".into(),
                message: "original action error".into()
            })
        );
        assert_eq!(
            backend.cleanup_error(),
            Some(Error::NativeFailure {
                code: "focus_restore_failed".into(),
                message: "restore failed".into()
            })
        );
        assert_eq!(
            native_cleanup_error(
                &json!({"cleanup_error":{"code":"cleanup_failed","message":"CUA release failed"}})
            ),
            Some(Error::NativeFailure {
                code: "cleanup_failed".into(),
                message: "CUA release failed".into()
            })
        );
    }

    fn scene_fixture(code: &str) -> Result<Worker, Error> {
        Worker::spawn(std::path::Path::new("python3"), &["-u", "-c", code]).map_err(transport_error)
    }

    fn echo_scene() -> Result<Worker, Error> {
        scene_fixture(
            "import sys\nfor line in sys.stdin:\n sys.stdout.write(line); sys.stdout.flush()",
        )
    }

    fn assert_gone(worker: &Worker) {
        assert_eq!(unsafe { libc::kill(worker.id() as i32, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[test]
    fn scene_owner_reaps_before_a_new_scene_can_start() {
        let owner = SceneOwner::default();
        let scene = owner.start(echo_scene).unwrap();
        let worker = scene.worker.clone();
        assert!(matches!(owner.start(echo_scene), Err(Error::Busy)));
        scene.finish().unwrap();
        assert_gone(&worker);
        let next = owner.start(echo_scene).unwrap();
        let next_worker = next.worker.clone();
        drop(next); // Unwinding/early return uses the same cleanup path.
        assert_gone(&next_worker);
        assert!(owner.state.lock().unwrap().worker.is_none());
    }

    #[test]
    fn closing_scene_owner_reaps_and_prevents_late_spawn() {
        let owner = SceneOwner::default();
        let scene = owner.start(echo_scene).unwrap();
        let worker = scene.worker.clone();
        owner.close().unwrap();
        assert_gone(&worker);
        assert!(matches!(
            owner.start(|| panic!("closed owner spawned")),
            Err(Error::Closed)
        ));
        drop(scene);
    }

    #[test]
    fn close_racing_spawn_cannot_lose_the_published_child() {
        use std::sync::mpsc;
        let owner = Arc::new(SceneOwner::default());
        let (started, starting) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (closing, closed) = mpsc::channel();
        let starter_owner = owner.clone();
        let starter = std::thread::spawn(move || {
            let scene = starter_owner
                .start(|| {
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                    echo_scene()
                })
                .unwrap();
            let worker = scene.worker.clone();
            // Keep the child registered until the concurrent close consumes it.
            closed.recv().unwrap();
            assert_gone(&worker);
            drop(scene);
        });
        starting.recv().unwrap();
        let closer_owner = owner.clone();
        let closer = std::thread::spawn(move || {
            closer_owner.close().unwrap();
            closing.send(()).unwrap();
        });
        release.send(()).unwrap();
        starter.join().unwrap();
        closer.join().unwrap();
        assert!(matches!(owner.check(), Err(Error::Closed)));
    }

    #[test]
    fn failed_scene_cleanup_keeps_owner_and_closes_admission() {
        let owner = SceneOwner::default();
        let scene = owner
            .start(|| scene_fixture("import sys\nsys.stdin.readline()\nsys.exit(74)"))
            .unwrap();
        let worker = scene.worker.clone();
        assert!(worker.call(b"{}", || Ok(Duration::from_secs(2))).is_err());
        let original = scene.finish().unwrap_err();
        assert_gone(&worker);
        assert!(owner
            .state
            .lock()
            .unwrap()
            .worker
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, &worker)));
        assert_eq!(owner.check(), Err(original.clone()));
        assert_eq!(owner.cleanup_error(), Some(original.clone()));
        assert_eq!(owner.close(), Err(original.clone()));
        assert!(owner
            .start(|| panic!("cleanup failure admitted a new child"))
            .is_err());
        let backend = NativeBackend {
            worker: Arc::new(echo_scene().unwrap()),
            scene: owner,
            cleanup_failure: Mutex::new(None),
            cursor: Arc::new(CursorSession::default()),
            cursor_operation: AtomicU64::new(0),
            focus_pending: AtomicBool::new(false),
            input_policy: InputPolicy::Auto,
            stamp: ProviderStamp {
                id: "cleanup-test".into(),
                generation: 1,
            },
        };
        assert_eq!(backend.cleanup_error(), Some(original.clone()));
        assert_eq!(backend.shutdown(), Err(original.clone()));
        assert_eq!(backend.cleanup_error(), Some(original));
    }

    #[test]
    fn resident_worker_cleanup_error_is_visible_without_a_scene() {
        let backend = NativeBackend {
            worker: Arc::new(
                scene_fixture("import sys\nsys.stdin.readline()\nsys.exit(74)").unwrap(),
            ),
            scene: SceneOwner::default(),
            cleanup_failure: Mutex::new(None),
            cursor: Arc::new(CursorSession::default()),
            cursor_operation: AtomicU64::new(0),
            focus_pending: AtomicBool::new(false),
            input_policy: InputPolicy::Auto,
            stamp: ProviderStamp {
                id: "resident-cleanup-test".into(),
                generation: 1,
            },
        };
        assert_eq!(backend.cleanup_error(), None);
        assert!(backend
            .worker
            .call(b"{}", || Ok(Duration::from_secs(2)))
            .is_err());
        let original = transport_error(backend.worker.cleanup_error().unwrap());
        assert_eq!(backend.scene.cleanup_error(), None);
        assert_eq!(backend.cleanup_error(), Some(original.clone()));
        assert_eq!(backend.shutdown(), Err(original.clone()));
        assert_eq!(backend.cleanup_error(), Some(original));
    }

    fn selected_scene_inventory(targets: &[Value], associated: bool) -> Value {
        let n = targets.len();
        let same_pid = if associated { 0 } else { n };
        json!({"windows":targets.iter().map(|target| json!({"target":target,"title":"Fixture","onscreen":true})).collect::<Vec<_>>(),
            "coverage":{"scope":"selected_surface","associated_count":if associated { n } else { 0 },
                "association_complete":false,"association_probe_failures":0,"association_error_codes":[],
                "diagnostics":{"cg_app_raw":same_pid,"supported_count":same_pid,
                    "excluded_unavailable_surface":0,"excluded_zero_area":0,"invalid_metadata":0,"duplicate_id":0}}})
    }

    #[test]
    fn scene_binding_requests_only_the_selected_surface() {
        let original = json!({"app":{"identity":"example.app","pid":21,"generation":30},
            "native_id":42,"generation":55});
        let mut local = original.clone();
        local["generation"] = json!(99); // Scene generation is deliberately independent.
        for associated in [false, true] {
            let result = bind_scene_target(&original, |request| {
                assert_eq!(
                    request,
                    json!({"operation":"bind_window", "app":original["app"], "native_id":42})
                );
                Ok(selected_scene_inventory(&[local.clone()], associated))
            });
            assert_eq!(result, Ok(local.clone()));
        }
    }

    #[test]
    fn scene_binding_preserves_native_and_transport_errors_without_another_request() {
        let original = json!({"app":{"identity":"example.app","pid":21,"generation":30},
            "native_id":42,"generation":55});
        for error in [Error::TimedOut, Error::Cancelled, Error::TargetChanged] {
            assert_eq!(
                bind_scene_target(&original, |_| Err(error.clone())),
                Err(error)
            );
        }
        assert_eq!(
            bind_scene_target(&original, |_| Ok(
                json!({"error":"target_changed", "message":"Selected surface closed"})
            )),
            Err(Error::TargetChanged)
        );
        assert_eq!(
            bind_scene_target(&original, |_| Ok(
                json!({"error":{"code":"target_changed"}})
            )),
            Err(Error::InvalidObservation)
        );
        assert!(bind_scene_target(&json!({}), |_| panic!(
            "Malformed target cannot start scene discovery"
        ))
        .is_err());
    }

    #[test]
    fn scene_target_selection_requires_the_same_app_and_one_exact_native_id() {
        let original = json!({"app":{"identity":"example.app","pid":21,"generation":30},
            "native_id":42,"generation":55});
        let mut local = original.clone();
        local["generation"] = json!(99);
        let inventory = |targets: &[Value]| selected_scene_inventory(targets, false);
        assert_eq!(
            select_scene_target(&inventory(&[local.clone()]), &original),
            Ok(local.clone())
        );
        assert_eq!(
            select_scene_target(&inventory(&[]), &original),
            Err(Error::InvalidObservation)
        );
        assert_eq!(
            select_scene_target(&inventory(&[local.clone(), local.clone()]), &original),
            Err(Error::InvalidObservation)
        );
        for (field, value) in [
            ("identity", json!("other.app")),
            ("pid", json!(22)),
            ("generation", json!(31)),
        ] {
            let mut wrong_app = local.clone();
            wrong_app["app"][field] = value;
            assert_eq!(
                select_scene_target(&inventory(&[wrong_app]), &original),
                Err(Error::WrongApp)
            );
        }
        let mut wrong_window = local.clone();
        wrong_window["native_id"] = json!(43);
        assert_eq!(
            select_scene_target(&inventory(&[wrong_window.clone()]), &original),
            Err(Error::TargetChanged)
        );
        assert_eq!(
            select_scene_target(&inventory(&[local.clone(), wrong_window]), &original),
            Err(Error::InvalidObservation)
        );
        for field in ["native_id", "generation"] {
            let mut invalid = local.clone();
            invalid[field] = json!(0);
            assert_eq!(
                select_scene_target(&inventory(&[invalid]), &original),
                Err(Error::InvalidObservation)
            );
        }
        let selected = inventory(&[local]);
        assert!(parse_window_inventory(&selected, &parse_app(&original["app"]).unwrap()).is_err());
        for scope in [
            "same_pid_application_surfaces",
            "application_and_proven_associated_surfaces",
        ] {
            let mut broad = selected.clone();
            broad["coverage"]["scope"] = json!(scope);
            assert_eq!(
                select_scene_target(&broad, &original),
                Err(Error::InvalidObservation)
            );
        }
        let mut partial = selected;
        partial["coverage"]["association_probe_failures"] = json!(1);
        partial["coverage"]["association_error_codes"] = json!([-25204]);
        assert_eq!(
            select_scene_target(&partial, &original),
            Err(Error::InvalidObservation)
        );
    }

    #[test]
    fn inventory_requires_structured_evidence_and_never_interprets_message_text() {
        let mut coverage = json!({"scope":"same_pid_layer_zero_standard_ax_windows","complete":false,
            "window_server_count":6,"supported_count":0,"ax_id_errors":[],"diagnostics":{
                "cg_app_raw":6,"window_server_count":6,"cg_non_layer_zero":0,"cg_invalid_metadata":0,
                "ax_window_count":0,"excluded_scope":0,"ax_pid_unproven":0,"ax_role_unproven":0,
                "ax_subrole_unproven":0,"exact_id_unproven":0,"exact_id_zero":0,"cg_owner_unmatched":0,
                "duplicate_id":0,"supported_count":0,"excluded_non_window_role":0,"excluded_non_standard_subrole":0,
                "excluded_open_save_panel":0,"excluded_non_layer_zero":0}});
        let failure = |value: &Value| {
            reply_error(&json!({"error":"window_inventory_unproven",
            "message":"not used for decisions","details":{"inventory":value}}))
        };
        assert!(matches!(
            failure(&coverage),
            Some(Error::InventoryUnproven(_))
        ));
        assert!(parse_inventory_report(&coverage, true).is_err());
        coverage["window_server_count"] = json!(0);
        coverage["diagnostics"]["window_server_count"] = json!(0);
        coverage["diagnostics"]["cg_app_raw"] = json!(0);
        assert!(parse_inventory_report(&coverage, true).is_ok());
        assert!(matches!(failure(&coverage), Some(Error::BackendFailed(_))));
        coverage["diagnostics"]["ax_window_count"] = json!(1);
        coverage["diagnostics"]["excluded_scope"] = json!(1);
        coverage["diagnostics"]["excluded_non_standard_subrole"] = json!(1);
        assert_eq!(
            parse_inventory_report(&coverage, true)
                .unwrap()
                .excluded_count,
            1
        );
        for field in ["ax_pid_unproven", "ax_role_unproven", "cg_invalid_metadata"] {
            let mut invalid = coverage.clone();
            invalid["diagnostics"][field] = json!(1);
            assert!(parse_inventory_report(&invalid, true).is_err());
        }
        coverage["diagnostics"]["ax_window_count"] = json!("1");
        assert!(parse_inventory_report(&coverage, true).is_err());
        assert!(matches!(
            reply_error(
                &json!({"error":"window_inventory_unproven","message":"window_server_count=6"})
            ),
            Some(Error::BackendFailed(_))
        ));
    }

    #[test]
    fn native_conditions_preserve_codes_separately_from_prose_and_dispatch() {
        for code in [
            "permission_denied",
            "unsupported_modal",
            "future_native_condition",
        ] {
            let error = Error::NativeFailure {
                code: code.into(),
                message: "not a classification input".into(),
            };
            assert_eq!(
                reply_error(&json!({"error":code,"message":"not a classification input"})),
                Some(error.clone())
            );
            assert_eq!(
                action_reply(
                    &json!({"dispatch":"unknown","evidence":"unverified","error":code,"message":"not a classification input"})
                ),
                ActionOutcome::unknown(error)
            );
        }
    }

    #[test]
    fn semantic_fields_are_read_independently_and_invalid_action_claims_are_rejected() {
        let mut value = json!({"id":1,"label":"display string unrelated to role or value",
            "role":"text_field","name":"Search","value":"hello","enabled":true,"actions":[],"can_press":false});
        let element = parse_element(&value).unwrap();
        assert_eq!(element.role, "text_field");
        assert_eq!(element.name.as_deref(), Some("Search"));
        assert_eq!(element.value.as_deref(), Some("hello"));
        assert!(element.actions.is_empty());
        value["enabled"] = Value::Null;
        assert_eq!(parse_element(&value).unwrap().enabled, None);
        value["can_press"] = json!(true);
        assert_eq!(parse_element(&value), Err(Error::InvalidObservation));
        value["actions"] = json!(["press"]);
        assert_eq!(parse_element(&value), Err(Error::InvalidObservation));
        value["enabled"] = json!(true);
        assert_eq!(parse_element(&value).unwrap().actions, vec!["press"]);
        value["actions"] = json!(["set_value"]);
        assert_eq!(parse_element(&value), Err(Error::InvalidObservation));
        value["actions"] = json!(["press"]);
        value.as_object_mut().unwrap().remove("role");
        assert_eq!(parse_element(&value), Err(Error::InvalidObservation));
    }

    #[test]
    fn native_discovery_pages_preserve_partial_progress_without_claiming_absence() {
        let mut value = json!({
            "target":{"app":{"identity":"test","pid":2,"generation":3},"native_id":4,"generation":5},
            "elements":[],
            "discovery":{"has_more":true,"complete":false,"reason":null,"visited":256}
        });
        let page = parse_discovery_page(&value).unwrap();
        assert!(page.elements.is_empty());
        assert!(page.discovery.has_more);
        assert!(!page.discovery.complete);
        assert_eq!(page.discovery.visited, 256);

        value["discovery"]["has_more"] = json!(false);
        assert!(parse_discovery_page(&value).is_err());
        value["discovery"]["reason"] = json!("scan_limit");
        assert_eq!(
            parse_discovery_page(&value)
                .unwrap()
                .discovery
                .reason
                .as_deref(),
            Some("scan_limit")
        );
        value["discovery"]["complete"] = json!(true);
        assert!(parse_discovery_page(&value).is_err());
        value["discovery"]["reason"] = Value::Null;
        assert!(parse_discovery_page(&value).unwrap().discovery.complete);
        value["discovery"]["visited"] = json!(8193);
        assert!(parse_discovery_page(&value).is_err());
    }

    #[test]
    fn native_discovery_rejects_oversized_rows_before_retaining_them() {
        let row = json!({"id":1,"label":"x".repeat(12 * 1024),"role":"button", "name":null,
            "value":null,"enabled":true,"actions":["press"],"can_press":true});
        let value = json!({
            "target":{"app":{"identity":"test","pid":2,"generation":3},"native_id":4,"generation":5},
            "elements":[row],
            "discovery":{"has_more":true,"complete":false,"reason":null,"visited":1}
        });
        assert!(parse_discovery_page(&value).is_err());
    }

    #[test]
    fn native_discovery_geometry_gap_preserves_rows_and_partial_page_evidence() {
        let mut value = json!({
            "target":{"app":{"identity":"test","pid":2,"generation":3},"native_id":4,"generation":5},
            "elements":[{"id":8,"label":"Known field","role":"text_field",
                "name":"Known field","value":"retained text","enabled":true,
                "actions":["setValue"],"can_press":false}],
            "discovery":{"has_more":true,"complete":false,
                "reason":"geometry_unavailable","visited":320}
        });
        for has_more in [true, false] {
            value["discovery"]["has_more"] = json!(has_more);
            let page = parse_discovery_page(&value).unwrap();
            assert_eq!(page.elements.len(), 1);
            assert_eq!(page.elements[0].id, ElementId(8));
            assert_eq!(page.elements[0].value.as_deref(), Some("retained text"));
            assert_eq!(page.discovery.has_more, has_more);
            assert!(!page.discovery.complete);
            assert_eq!(
                page.discovery.reason.as_deref(),
                Some("geometry_unavailable")
            );
            assert_eq!(page.discovery.visited, 320);
        }
        value["discovery"]["complete"] = json!(true);
        assert!(parse_discovery_page(&value).is_err());
        value["discovery"]["complete"] = json!(false);
        value["discovery"]["reason"] = json!("unrecognized_native_reason");
        assert!(parse_discovery_page(&value).is_err());
    }

    #[test]
    fn shutdown_failure_is_not_hidden_by_context_cancellation() {
        let error = TransportError {
            kind: TransportErrorKind::Shutdown("kill failed".into()),
            request_may_have_been_sent: true,
        };
        assert_eq!(
            transport_cause(error, Some(Error::Cancelled)),
            Error::BackendFailed("native transport: Shutdown(\"kill failed\")".into())
        );
        let control = TransportError {
            kind: TransportErrorKind::Control(StopReason::Cancelled),
            request_may_have_been_sent: true,
        };
        assert_eq!(
            transport_cause(control, Some(Error::TimedOut)),
            Error::TimedOut
        );
    }
    #[test]
    fn late_or_unassociated_responses_are_rejected() {
        assert_eq!(
            envelope_reply(
                &json!({"request_id":8,"result":{"dispatch":"dispatched"}}),
                9
            ),
            Err(Error::InvalidObservation)
        );
        assert_eq!(
            envelope_reply(&json!({"result":{}}), 9),
            Err(Error::InvalidObservation)
        );
        assert_eq!(
            envelope_reply(&json!({"request_id":9,"result":null}), 9),
            Err(Error::InvalidObservation)
        );
        assert_eq!(
            envelope_reply(&json!({"request_id":9,"result":{}}), 9),
            Ok(json!({}))
        );
    }
    #[test]
    fn surface_inventory_is_ax_free_and_rejects_missing_or_conflicting_evidence() {
        let mut value = json!({"scope":"same_pid_application_surfaces","diagnostics":{
            "cg_app_raw":4,"supported_count":2,"excluded_unavailable_surface":1,
            "excluded_zero_area":1,"invalid_metadata":0,"duplicate_id":0}});
        let report = parse_inventory_report(&value, true).unwrap();
        assert_eq!(report.scope, InventoryScope::SameProcessApplicationSurfaces);
        assert_eq!(
            (
                report.candidate_count,
                report.supported_count,
                report.excluded_count
            ),
            (4, 2, 2)
        );
        assert_eq!(report.diagnostics.provider, "macos.window-server");
        assert!(parse_inventory_report(&value, false).is_err());
        value["diagnostics"]["supported_count"] = json!(1);
        value["diagnostics"]["invalid_metadata"] = json!(1);
        assert!(parse_inventory_report(&value, true).is_err());
        assert!(parse_inventory_report(&value, false).is_ok());
        value["diagnostics"]["extra"] = json!(0);
        assert!(parse_inventory_report(&value, false).is_err());
    }

    fn associated_inventory_fixture() -> Value {
        // NativeStore.listWindows keeps the six same-PID diagnostics unchanged;
        // only individually proven associated rows are added outside that sum.
        json!({"windows":[
            {"target":{"app":{"identity":"example.app","pid":21,"generation":30},
                "native_id":42,"generation":55},"title":"Document","onscreen":true},
            {"target":{"app":{"identity":"example.app","pid":21,"generation":30},
                "native_id":43,"generation":56},"title":"Save","onscreen":true}],
            "coverage":{"scope":"application_and_proven_associated_surfaces",
                "associated_count":1,"association_complete":false,
                "association_probe_failures":0,"association_error_codes":[],"diagnostics":{
                    "cg_app_raw":3,"supported_count":1,"excluded_unavailable_surface":1,
                    "excluded_zero_area":1,"invalid_metadata":0,"duplicate_id":0}}})
    }

    #[test]
    fn associated_inventory_matches_native_shape_without_claiming_all_helpers() {
        let mut value = associated_inventory_fixture();
        let app = parse_app(&value["windows"][0]["target"]["app"]).unwrap();
        let inventory = parse_window_inventory(&value, &app).unwrap();
        assert_eq!(
            inventory.report.scope,
            InventoryScope::ApplicationAndProvenAssociatedSurfaces {
                associated_count: 1
            }
        );
        assert_eq!(
            (
                inventory.report.candidate_count,
                inventory.report.supported_count,
                inventory.report.excluded_count
            ),
            (4, 2, 2)
        );
        assert_eq!(inventory.windows.len(), 2);
        assert_eq!(
            inventory
                .report
                .diagnostics
                .counters
                .iter()
                .find(|(name, _)| name == "supported_count")
                .unwrap()
                .1,
            1
        );
        assert_eq!(
            select_scene_target(&value, &value["windows"][1]["target"]),
            Err(Error::InvalidObservation) // Full discovery cannot bind a selected scene.
        );

        // Ordinary applications use the same new shape with no associated rows.
        value["coverage"]["associated_count"] = json!(0);
        value["windows"].as_array_mut().unwrap().pop();
        assert_eq!(
            parse_window_inventory(&value, &app)
                .unwrap()
                .report
                .supported_count,
            1
        );
        value["coverage"]["association_probe_failures"] = json!(1);
        value["coverage"]["association_error_codes"] = json!([-25204]);
        let report = parse_window_inventory(&value, &app).unwrap().report;
        assert_eq!(report.diagnostics.error_codes, vec![-25204]);
        assert!(report
            .diagnostics
            .counters
            .contains(&("association_probe_failures".into(), 1)));
    }

    #[test]
    fn associated_inventory_rejects_malformed_counts_and_unbound_rows() {
        let original = associated_inventory_fixture();
        let app = parse_app(&original["windows"][0]["target"]["app"]).unwrap();
        for invalid_count in [Value::Null, json!(-1), json!(1.5), json!("1"), json!(129)] {
            let mut value = original.clone();
            value["coverage"]["associated_count"] = invalid_count;
            assert!(parse_window_inventory(&value, &app).is_err());
        }
        for mutate in [
            |v: &mut Value| {
                v["coverage"]["association_complete"] = json!(true);
            },
            |v: &mut Value| {
                v["coverage"]
                    .as_object_mut()
                    .unwrap()
                    .remove("association_complete");
            },
            |v: &mut Value| {
                v["coverage"]["extra"] = json!(0);
            },
            |v: &mut Value| {
                v["coverage"]
                    .as_object_mut()
                    .unwrap()
                    .remove("association_probe_failures");
            },
            |v: &mut Value| {
                v["coverage"]["association_probe_failures"] = json!(1);
            },
            |v: &mut Value| {
                v["coverage"]["association_error_codes"] = json!([0]);
            },
            |v: &mut Value| {
                v["coverage"]["association_probe_failures"] = json!(1_000_001);
            },
            |v: &mut Value| {
                v["coverage"]["diagnostics"]["cg_app_raw"] = json!(4);
            },
            |v: &mut Value| {
                v["coverage"]["associated_count"] = json!(0);
            },
            |v: &mut Value| {
                v["coverage"]["associated_count"] = json!(2);
            },
            |v: &mut Value| {
                v["windows"].as_array_mut().unwrap().pop();
            },
            |v: &mut Value| {
                v["windows"][1]["target"]["native_id"] = json!(42);
            },
            |v: &mut Value| {
                v["windows"][1]["target"]["native_id"] = json!(0);
            },
            |v: &mut Value| {
                v["windows"][1]["target"]["generation"] = json!(0);
            },
            |v: &mut Value| {
                v["windows"][1]["target"]["app"]["pid"] = json!(22);
            },
        ] {
            let mut value = original.clone();
            mutate(&mut value);
            assert!(parse_window_inventory(&value, &app).is_err(), "{value}");
        }
        let mut limit = original["coverage"].clone();
        limit["associated_count"] = json!(128);
        assert!(parse_inventory_report(&limit, true).is_err()); // 129 supported rows
    }

    #[test]
    fn native_text_range_error_keeps_the_reference_preserving_core_identity() {
        for (code, expected) in [
            (
                "invalid_text_range",
                Error::NativeFailure {
                    code: "invalid_text_range".into(),
                    message: "Native rejection".into(),
                },
            ),
            ("unsupported_capability", Error::UnsupportedCapability),
            ("stale_observation", Error::StaleObservation),
        ] {
            assert_eq!(
                reply_error(&json!({"error":code,"message":"Native rejection"})),
                Some(expected)
            );
        }
    }

    #[test]
    fn set_value_claim_is_independent_of_press_and_requires_enabled_evidence() {
        let mut element = json!({"id":1,"label":"Text","role":"text_field","name":"Text",
            "value":"old","enabled":true,"actions":["setValue"],"can_press":false});
        assert_eq!(parse_element(&element).unwrap().actions, vec!["setValue"]);
        element["enabled"] = Value::Null;
        assert!(parse_element(&element).is_err());
        element["enabled"] = json!(true);
        element["actions"] = json!(["setValue", "setValue"]);
        assert!(parse_element(&element).is_err());
        element["actions"] = json!(["press", "setValue"]);
        element["can_press"] = json!(true);
        assert!(parse_element(&element).is_ok());
    }

    #[test]
    fn uncertain_or_malformed_native_replies_never_prove_no_dispatch() {
        for reply in [
            json!({}),
            json!({"dispatch":"not_dispatched"}),
            json!({"dispatch":"not_dispatched","evidence":"unverified","error":12}),
            json!({"dispatch":"dispatched","evidence":"unverified","error":"timeout"}),
        ] {
            assert_eq!(
                action_reply(&reply),
                ActionOutcome::unknown(Error::InvalidObservation)
            );
        }
        let rejected = json!({"dispatch":"not_dispatched","evidence":"unverified","error":"stale_observation"});
        assert_eq!(
            action_reply(&rejected),
            ActionOutcome::rejected(Error::StaleObservation)
        );
        assert_eq!(
            action_reply(
                &json!({"dispatch":"unknown","evidence":"unverified","error":"deadline_exceeded"})
            ),
            ActionOutcome::unknown(Error::TimedOut)
        );
    }
}
fn unique_generation() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .expect("provider identity exhausted")
}
fn failed(error: impl std::fmt::Display) -> Error {
    Error::BackendFailed(error.to_string())
}
fn transport_cause(error: TransportError, context: Option<Error>) -> Error {
    if matches!(error.kind, TransportErrorKind::Control(_)) {
        context.unwrap_or_else(|| transport_error(error))
    } else {
        transport_error(error)
    }
}
fn transport_error(error: TransportError) -> Error {
    match error.kind {
        TransportErrorKind::Control(StopReason::TimedOut) => Error::TimedOut,
        TransportErrorKind::Control(StopReason::Cancelled) => Error::Cancelled,
        kind => failed(format!("native transport: {kind:?}")),
    }
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str, Error> {
    v[key].as_str().ok_or(Error::InvalidObservation)
}
fn number(v: &Value, key: &str) -> Result<u64, Error> {
    v[key].as_u64().ok_or(Error::InvalidObservation)
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, Error> {
    v[key].as_array().ok_or(Error::InvalidObservation)
}
fn optional_text(value: &Value, key: &str) -> Result<Option<String>, Error> {
    match value.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.chars().count() <= 256 => Ok(Some(text.clone())),
        _ => Err(Error::InvalidObservation),
    }
}
fn parse_discovery_page(value: &Value) -> Result<BackendObservationPage, Error> {
    let rows = array(value, "elements")?;
    if rows.len() > 100 || serde_json::to_vec(rows).map_err(failed)?.len() > 12 * 1024 {
        return Err(Error::InvalidObservation);
    }
    let page = &value["discovery"];
    let has_more = page["has_more"]
        .as_bool()
        .ok_or(Error::InvalidObservation)?;
    let complete = page["complete"]
        .as_bool()
        .ok_or(Error::InvalidObservation)?;
    let visited = u32::try_from(number(page, "visited")?).map_err(failed)?;
    let reason = optional_text(page, "reason")?;
    if visited > 8_192
        || rows.len() > visited as usize
        || (complete && (has_more || reason.is_some()))
        || (!complete && !has_more && reason.is_none())
        || reason.as_ref().is_some_and(|reason| {
            !matches!(
                reason.as_str(),
                "scan_limit"
                    | "depth_limit"
                    | "frontier_limit"
                    | "node_unavailable"
                    | "children_unavailable"
                    | "ownership_unproven"
                    | "tree_changed"
                    | "text_limit"
                    | "unsupported_semantic_tree"
                    | "content_unproven"
                    | "geometry_unavailable"
            )
        })
    {
        return Err(Error::InvalidObservation);
    }
    Ok(BackendObservationPage {
        target: parse_target(&value["target"])?,
        elements: rows.iter().map(parse_element).collect::<Result<_, _>>()?,
        discovery: DiscoveryPage {
            has_more,
            complete,
            reason,
            visited,
        },
    })
}
fn parse_element(value: &Value) -> Result<Element, Error> {
    let can_press = value["can_press"]
        .as_bool()
        .ok_or(Error::InvalidObservation)?;
    let actions = array(value, "actions")?;
    if actions.len() > 2
        || actions
            .iter()
            .any(|action| action != "press" && action != "setValue")
        || actions
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != actions.len()
        || can_press != actions.iter().any(|action| action == "press")
    {
        return Err(Error::InvalidObservation);
    }
    let enabled = match value.get("enabled") {
        Some(Value::Null) => None,
        Some(Value::Bool(value)) => Some(*value),
        _ => return Err(Error::InvalidObservation),
    };
    if !actions.is_empty() && enabled != Some(true) {
        return Err(Error::InvalidObservation);
    }
    Ok(Element {
        id: ElementId(number(value, "id")?),
        label: string(value, "label")?.into(),
        role: string(value, "role")?.into(),
        name: optional_text(value, "name")?,
        value: optional_text(value, "value")?,
        enabled,
        actions: actions
            .iter()
            .map(|action| action.as_str().unwrap().into())
            .collect(),
        can_press,
    })
}
fn app_json(app: &AppInstance) -> Value {
    json!({"identity":app.identity.0,"pid":app.pid,"generation":app.generation})
}
fn target_json(target: &WindowTarget) -> Value {
    json!({"app":app_json(&target.app),"native_id":target.native_id,"generation":target.generation})
}
fn parse_app(v: &Value) -> Result<AppInstance, Error> {
    Ok(AppInstance {
        identity: AppIdentity(string(v, "identity")?.into()),
        pid: u32::try_from(number(v, "pid")?).map_err(failed)?,
        generation: number(v, "generation")?,
    })
}
fn parse_target(v: &Value) -> Result<WindowTarget, Error> {
    Ok(WindowTarget {
        app: parse_app(&v["app"])?,
        native_id: number(v, "native_id")?,
        generation: number(v, "generation")?,
    })
}
fn reply_error(v: &Value) -> Option<Error> {
    v["error"].as_str().map(|code| match code {
        "window_inventory_unproven" => {
            match parse_inventory_report(&v["details"]["inventory"], false) {
                Ok(report) => Error::InventoryUnproven(report),
                Err(error) => error,
            }
        }
        "target_changed" | "app_closed" | "window_closed" => Error::TargetChanged,
        "stale_observation" => Error::StaleObservation,
        "timed_out" | "deadline_exceeded" => Error::TimedOut,
        "unsupported_capability" | "unsupported_platform" => Error::UnsupportedCapability,
        "invalid_point" => Error::InvalidPoint,
        _ => Error::NativeFailure {
            code: code.into(),
            message: v["message"].as_str().unwrap_or("native error").into(),
        },
    })
}

fn parse_inventory_report(value: &Value, proven: bool) -> Result<InventoryReport, Error> {
    if matches!(
        value["scope"].as_str(),
        Some("same_pid_application_surfaces" | "application_and_proven_associated_surfaces")
    ) {
        return parse_surface_inventory(value, proven);
    }
    const COUNTERS: [&str; 18] = [
        "cg_app_raw",
        "window_server_count",
        "cg_non_layer_zero",
        "cg_invalid_metadata",
        "ax_window_count",
        "excluded_scope",
        "ax_pid_unproven",
        "ax_role_unproven",
        "ax_subrole_unproven",
        "exact_id_unproven",
        "exact_id_zero",
        "cg_owner_unmatched",
        "duplicate_id",
        "supported_count",
        "excluded_non_window_role",
        "excluded_non_standard_subrole",
        "excluded_open_save_panel",
        "excluded_non_layer_zero",
    ];
    let invalid = || failed("Malformed native inventory evidence");
    if value["scope"] != "same_pid_layer_zero_standard_ax_windows" || value["complete"] != false {
        return Err(invalid());
    }
    let counts = value["diagnostics"].as_object().ok_or_else(invalid)?;
    if counts.len() != COUNTERS.len() {
        return Err(invalid());
    }
    let counters = COUNTERS
        .iter()
        .map(|name| {
            let count = counts
                .get(*name)
                .and_then(Value::as_u64)
                .filter(|n| *n <= 1_000_000)
                .ok_or_else(invalid)?;
            Ok(((*name).into(), count))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let count = |name: &str| counts[name].as_u64().unwrap();
    let codes = value["ax_id_errors"]
        .as_array()
        .filter(|a| a.len() <= 8)
        .ok_or_else(invalid)?;
    let error_codes = codes
        .iter()
        .map(|code| {
            code.as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .filter(|n| *n != 0)
                .ok_or_else(invalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let report = InventoryReport {
        scope: InventoryScope::SameProcessStandardWindows,
        candidate_count: count("ax_window_count") as u32,
        supported_count: count("supported_count") as u32,
        excluded_count: count("excluded_scope") as u32,
        diagnostics: InventoryDiagnostics {
            provider: "macos.ax-cg".into(),
            counters,
            error_codes,
        },
    };
    let unresolved = COUNTERS[6..13].iter().any(|name| count(name) > 0)
        || count("cg_invalid_metadata") > 0
        || (report.candidate_count == 0 && count("window_server_count") > 0);
    if !report.valid()
        || proven == unresolved
        || COUNTERS[6..14].iter().map(|name| count(name)).sum::<u64>() + count("excluded_scope")
            != count("ax_window_count")
        || COUNTERS[14..].iter().map(|name| count(name)).sum::<u64>() != count("excluded_scope")
        || count("window_server_count") + count("cg_non_layer_zero") + count("cg_invalid_metadata")
            != count("cg_app_raw")
        || report.diagnostics.error_codes.len() as u64 != count("exact_id_unproven").min(8)
        || value["supported_count"].as_u64() != Some(count("supported_count"))
        || value["window_server_count"].as_u64() != Some(count("window_server_count"))
        || (proven
            && (report.supported_count + report.excluded_count != report.candidate_count
                || !report.diagnostics.error_codes.is_empty()))
    {
        return Err(invalid());
    }
    Ok(report)
}

fn parse_surface_inventory(value: &Value, proven: bool) -> Result<InventoryReport, Error> {
    const NAMES: [&str; 6] = [
        "cg_app_raw",
        "supported_count",
        "excluded_unavailable_surface",
        "excluded_zero_area",
        "invalid_metadata",
        "duplicate_id",
    ];
    let invalid = || failed("Malformed native surface inventory evidence");
    let fields = value.as_object().ok_or_else(invalid)?;
    let (associated, association_failures, error_codes) = if matches!(
        value["scope"].as_str(),
        Some("application_and_proven_associated_surfaces" | "selected_surface")
    ) {
        if fields.len() != 6 || value["association_complete"] != false {
            return Err(invalid());
        }
        let count = value["associated_count"]
            .as_u64()
            .filter(|count| *count <= 128)
            .ok_or_else(invalid)? as u32;
        let failures = value["association_probe_failures"]
            .as_u64()
            .filter(|count| *count <= 1_000_000)
            .ok_or_else(invalid)?;
        let codes = value["association_error_codes"]
            .as_array()
            .filter(|codes| codes.len() as u64 == failures.min(8))
            .ok_or_else(invalid)?
            .iter()
            .map(|code| {
                code.as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .filter(|n| *n != 0)
                    .ok_or_else(invalid)
            })
            .collect::<Result<Vec<_>, _>>()?;
        (Some(count), failures, codes)
    } else {
        if fields.len() != 2 {
            return Err(invalid());
        }
        (None, 0, vec![])
    };
    let counts = value["diagnostics"]
        .as_object()
        .filter(|v| v.len() == NAMES.len())
        .ok_or_else(invalid)?;
    let mut counters = NAMES
        .iter()
        .map(|name| {
            let n = counts
                .get(*name)
                .and_then(Value::as_u64)
                .filter(|n| *n <= 1_000_000)
                .ok_or_else(invalid)?;
            Ok(((*name).into(), n))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    if associated.is_some() {
        counters.push(("association_probe_failures".into(), association_failures));
    }
    let count = |name: &str| counts[name].as_u64().unwrap();
    let unresolved = count("invalid_metadata") + count("duplicate_id");
    let excluded = count("excluded_unavailable_surface") + count("excluded_zero_area");
    if proven != (unresolved == 0)
        || count("supported_count") + excluded + unresolved != count("cg_app_raw")
    {
        return Err(invalid());
    }
    let report = InventoryReport {
        scope: match associated {
            Some(associated_count) => {
                InventoryScope::ApplicationAndProvenAssociatedSurfaces { associated_count }
            }
            None => InventoryScope::SameProcessApplicationSurfaces,
        },
        candidate_count: count("cg_app_raw") as u32 + associated.unwrap_or(0),
        supported_count: count("supported_count") as u32 + associated.unwrap_or(0),
        excluded_count: excluded as u32,
        diagnostics: InventoryDiagnostics {
            provider: "macos.window-server".into(),
            counters,
            error_codes,
        },
    };
    if !report.valid() || (associated.is_some() && report.supported_count > 128) {
        return Err(invalid());
    }
    Ok(report)
}

fn parse_window_inventory(value: &Value, app: &AppInstance) -> Result<WindowInventory, Error> {
    let report = parse_inventory_report(&value["coverage"], true)?;
    let windows = parse_window_rows(value, app, report.supported_count)?;
    Ok(WindowInventory { windows, report })
}

fn parse_window_state_probe(value: &Value, app: &AppInstance) -> Result<WindowStateProbe, Error> {
    match string(value, "status")? {
        "pending" => {
            let reason = string(value, "reason")?;
            if ![
                "ax_busy",
                "blocked_by_modal",
                "semantic_window_unavailable",
                "focus_changed",
                "window_present",
                "window_not_visible",
                "dialog_not_present",
                "dialog_not_ready",
                "ambiguous_dialog",
            ]
            .contains(&reason)
            {
                return Err(Error::InvalidObservation);
            }
            Ok(WindowStateProbe::Pending {
                reason: reason.into(),
            })
        }
        "closed" => Ok(WindowStateProbe::Closed),
        "matched" => {
            let mut windows = parse_window_rows(value, app, 1)?;
            let kind = match string(value, "kind")? {
                "window" => WindowKind::Window,
                "dialog" => WindowKind::Dialog,
                "sheet" => WindowKind::Sheet,
                _ => return Err(Error::InvalidObservation),
            };
            let relation = match string(value, "relation")? {
                "same_window" => WindowRelation::SameWindow,
                "child_modal" => WindowRelation::ChildModal,
                "application_modal" => WindowRelation::ApplicationModal,
                _ => return Err(Error::InvalidObservation),
            };
            Ok(WindowStateProbe::Window(MatchedWindow {
                window: windows.remove(0),
                kind,
                relation,
            }))
        }
        _ => Err(Error::InvalidObservation),
    }
}

fn parse_window_rows(
    value: &Value,
    app: &AppInstance,
    supported_count: u32,
) -> Result<Vec<WindowInfo>, Error> {
    let rows = array(value, "windows")?;
    if rows.len() > 256 || rows.len() != supported_count as usize {
        return Err(failed("Native inventory row/count mismatch"));
    }
    let mut seen = std::collections::HashSet::new();
    rows.iter()
        .map(|row| {
            let target = parse_target(&row["target"])?;
            if &target.app != app {
                return Err(Error::WrongApp);
            }
            if target.native_id == 0
                || target.native_id > u64::from(u32::MAX)
                || target.generation == 0
                || !seen.insert(target.native_id)
            {
                return Err(Error::InvalidObservation);
            }
            Ok(WindowInfo {
                target,
                title: string(row, "title")?.into(),
                visible: row["onscreen"].as_bool().ok_or(Error::InvalidObservation)?,
            })
        })
        .collect()
}

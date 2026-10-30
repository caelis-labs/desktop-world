//! Narrow macOS provider. All native objects and SCK clients belong to its worker.
use super::process::{StopReason, TransportError, TransportErrorKind, Worker};
use crate::computer::*;
use base64::Engine;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
mod cua_input;
mod worker;
pub use worker::worker_entry;

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
}

#[derive(Default)]
struct SceneState {
    closed: bool,
    worker: Option<Arc<Worker>>,
    failure: Option<Error>,
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
        }
        result
    }

    fn close(&self) -> Result<(), Error> {
        let worker = {
            let mut state = self.state.lock().unwrap();
            state.closed = true;
            state.worker.clone()
        };
        if let Some(worker) = worker {
            self.finish(&worker)?;
        }
        let state = self.state.lock().unwrap();
        state.failure.clone().map_or(Ok(()), Err)
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
    "same-PID available application surfaces, including raised panels; AX semantics are a separate exact-window capability";
const CONDITIONS: &[&str] = &[
    "semantic and input operations require macOS Accessibility permission",
    "capture and click require macOS Screen Recording permission",
    "input prefers directed background delivery; a side-effect-free preflight decline may use temporary foreground focus",
    "foreground coordinate fallback requires a visible, unobstructed point in the captured window",
    "keyboard, focus and semantic input require an exact standard AX window without modal interception",
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
        )
    }
    pub fn description() -> Value {
        json!({"provider":"dtw-macos-native", "coverage":COVERAGE,"conditions":CONDITIONS,
            "supported_capabilities":Capability::ALL.into_iter().filter(|c|Self::supports_capability(*c)).map(Capability::as_str).collect::<Vec<_>>()})
    }

    pub fn spawn() -> Result<Self, Error> {
        Self::spawn_with_policy(InputPolicy::Auto)
    }

    pub fn spawn_with_policy(input_policy: InputPolicy) -> Result<Self, Error> {
        let exe = std::env::current_exe().map_err(failed)?;
        let worker = Worker::spawn(&exe, &["--dtw-native-worker"]).map_err(transport_error)?;
        Ok(Self {
            worker: Arc::new(worker),
            scene: SceneOwner::default(),
            cleanup_failure: Mutex::new(None),
            focus_pending: AtomicBool::new(false),
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
        mut request: Value,
        call: &CallContext<'_>,
    ) -> Result<Value, (Error, bool)> {
        let request_id = unique_generation();
        request["request_id"] = json!(request_id);
        request["input_policy"] = json!(self.input_policy.as_str());
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
            .call(request.to_string().as_bytes(), || match call.remaining() {
                Ok(left) => {
                    let left = left.min(deadline.saturating_duration_since(Instant::now()));
                    if left.is_zero() {
                        Err(StopReason::TimedOut)
                    } else {
                        Ok(left)
                    }
                }
                Err(error) => {
                    *stop.lock().unwrap() = Some(error);
                    Err(StopReason::Cancelled)
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
        Ok(reply)
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
        if explicit_focus
            && outcome.dispatch == DispatchState::NotDispatched
            && !outcome
                .delivery
                .as_ref()
                .is_some_and(|value| value.foreground_borrowed)
        {
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
        Ok(reply["bounds"].clone())
    }

    fn scene_rpc(
        &self,
        mut request: Value,
        call: &CallContext<'_>,
    ) -> Result<Value, (Error, bool)> {
        let original = request["target"].clone();
        // This first validation rejects stale refs before spawning a scene.
        self.validate_window(&original, call)
            .map_err(|error| (error, false))?;
        let scene = self
            .scene
            .start(|| {
                let exe = std::env::current_exe().map_err(failed)?;
                Worker::spawn(&exe, &["--dtw-scene-worker"]).map_err(transport_error)
            })
            .map_err(|error| (error, false))?;
        let result = (|| {
            // A scene retains its own AX window object. Revalidate the resident
            // binding AFTER that object is acquired, linking both identities.
            // A reused numeric ID cannot silently replace either retained window.
            let inventory = self
                .rpc_on(
                    &scene.worker,
                    json!({"operation":"windows","app":original["app"]}),
                    call,
                )
                .map_err(|(error, _)| (error, false))?;
            if inventory
                .get("error")
                .is_some_and(|error| !error.is_string())
            {
                return Err((Error::InvalidObservation, false));
            }
            if let Some(error) = reply_error(&inventory) {
                return Err((error, false));
            }
            let local =
                select_scene_target(&inventory, &original).map_err(|error| (error, false))?;
            let before = self
                .validate_window(&original, call)
                .map_err(|error| (error, false))?;
            request["target"] = local.clone();
            let capture = request["operation"] == "capture";
            let mut reply = self.rpc_on(&scene.worker, request, call)?;
            if capture && reply.get("error").is_some_and(|error| !error.is_string()) {
                return Err((Error::InvalidObservation, false));
            }
            if capture && reply.get("error").is_none() {
                if reply["target"] != local {
                    return Err((Error::InvalidObservation, false));
                }
                let after = self
                    .validate_window(&original, call)
                    .map_err(|error| (error, false))?;
                if before != after || reply["bounds"] != after {
                    return Err((Error::StaleObservation, false));
                }
                reply["target"] = original;
            }
            Ok(reply)
        })();
        // The owner retains any cleanup failure for the independent diagnostic
        // channel. Never replace a native outcome with that later failure.
        let _ = scene.finish();
        result
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

fn select_scene_target(inventory: &Value, original: &Value) -> Result<Value, Error> {
    let original = parse_target(original)?;
    let report = parse_inventory_report(&inventory["coverage"], true)?;
    let rows = array(inventory, "windows")?;
    if rows.len() != report.supported_count as usize || rows.len() > 256 {
        return Err(Error::InvalidObservation);
    }
    let mut matched = None;
    for row in rows {
        let target = parse_target(&row["target"])?;
        if target.app != original.app {
            return Err(Error::WrongApp);
        }
        if target.native_id == original.native_id {
            if matched.is_some() {
                return Err(Error::InvalidObservation);
            }
            matched = Some(row["target"].clone());
        }
    }
    matched.ok_or(Error::TargetChanged)
}
impl Backend for NativeBackend {
    type Binding = NativeBinding;
    fn finish_execution(&self) -> Result<Option<FocusReturn>, Error> {
        let until = Instant::now() + Duration::from_millis(400);
        let request_id = unique_generation();
        let request = json!({"operation":"release_focus","request_id":request_id,
            "deadline_unix_ms":(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()+Duration::from_millis(350)).as_millis() as u64});
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
        let report = parse_inventory_report(&result["coverage"], true)?;
        let rows = array(&result, "windows")?;
        if rows.len() > 256 || rows.len() != report.supported_count as usize {
            return Err(failed("Native inventory row/count mismatch"));
        }
        let windows = rows
            .iter()
            .map(|row| {
                Ok(WindowInfo {
                    target: parse_target(&row["target"])?,
                    title: string(row, "title")?.into(),
                    visible: row["onscreen"].as_bool().ok_or(Error::InvalidObservation)?,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(WindowInventory { windows, report })
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
        let reply = self.read_rpc(
            json!({"operation":"observe","target":target_json(target),
                "query":{"role":query.role,"text":query.text,"limit":query.limit}}),
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
        let reply = self
            .scene_rpc(
                json!({"operation":"capture","target":target_json(target)}),
                call,
            )
            .map_err(|(error, _)| error)?;
        if let Some(error) = reply_error(&reply) {
            return Err(error);
        }
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
        let screen_x =
            bounds[0].as_f64().unwrap() + point.x / *width as f64 * bounds[2].as_f64().unwrap();
        let screen_y =
            bounds[1].as_f64().unwrap() + point.y / *height as f64 * bounds[3].as_f64().unwrap();
        let scene = self.scene_rpc(json!({"operation":"click","target":target_json(target),"bounds":bounds,"screen_x":screen_x,"screen_y":screen_y,"button":match options.button {MouseButton::Left=>"left",MouseButton::Right=>"right"},"count":options.count}),call);
        match scene {
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
    fn action_keeps_native_route_and_return_without_claiming_effect() {
        for (route, borrowed, returned) in [
            ("semantic", false, None),
            ("background", false, None),
            ("foreground", true, Some("restored")),
            ("foreground", true, Some("changed_externally")),
            ("foreground", true, Some("pending")),
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

    #[test]
    fn scene_target_selection_requires_the_same_app_and_one_exact_native_id() {
        let original = json!({"app":{"identity":"example.app","pid":21,"generation":30},
            "native_id":42,"generation":55});
        let mut local = original.clone();
        local["generation"] = json!(99);
        let inventory = |targets: &[Value]| {
            let n = targets.len();
            json!({"windows":targets.iter().map(|target| json!({"target":target})).collect::<Vec<_>>(),
                "coverage":{"scope":"same_pid_layer_zero_standard_ax_windows","complete":false,
                    "window_server_count":n,"supported_count":n,"ax_id_errors":[],"diagnostics":{
                        "cg_app_raw":n,"window_server_count":n,"cg_non_layer_zero":0,"cg_invalid_metadata":0,
                        "ax_window_count":n,"excluded_scope":0,"ax_pid_unproven":0,"ax_role_unproven":0,
                        "ax_subrole_unproven":0,"exact_id_unproven":0,"exact_id_zero":0,"cg_owner_unmatched":0,
                        "duplicate_id":0,"supported_count":n,"excluded_non_window_role":0,"excluded_non_standard_subrole":0,
                        "excluded_open_save_panel":0,"excluded_non_layer_zero":0}}})
        };
        assert_eq!(
            select_scene_target(&inventory(&[local.clone()]), &original),
            Ok(local.clone())
        );
        assert_eq!(
            select_scene_target(&inventory(&[]), &original),
            Err(Error::TargetChanged)
        );
        assert_eq!(
            select_scene_target(&inventory(&[local.clone(), local.clone()]), &original),
            Err(Error::InvalidObservation)
        );
        let mut wrong_app = local;
        wrong_app["app"]["generation"] = json!(31);
        assert_eq!(
            select_scene_target(&inventory(&[wrong_app]), &original),
            Err(Error::WrongApp)
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
    if value["scope"] == "same_pid_application_surfaces" {
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
    let counts = value["diagnostics"]
        .as_object()
        .filter(|v| v.len() == NAMES.len())
        .ok_or_else(invalid)?;
    let counters = NAMES
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
    let count = |name: &str| counts[name].as_u64().unwrap();
    let unresolved = count("invalid_metadata") + count("duplicate_id");
    let excluded = count("excluded_unavailable_surface") + count("excluded_zero_area");
    if proven != (unresolved == 0)
        || count("supported_count") + excluded + unresolved != count("cg_app_raw")
    {
        return Err(invalid());
    }
    let report = InventoryReport {
        scope: InventoryScope::SameProcessApplicationSurfaces,
        candidate_count: count("cg_app_raw") as u32,
        supported_count: count("supported_count") as u32,
        excluded_count: excluded as u32,
        diagnostics: InventoryDiagnostics {
            provider: "macos.window-server".into(),
            counters,
            error_codes: vec![],
        },
    };
    if !report.valid() {
        return Err(invalid());
    }
    Ok(report)
}

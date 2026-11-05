//! Bounded DTW protocol shared by the Windows and experimental X11 adapters.
//! Native objects never leave their dedicated process; an interrupted call is
//! never replayed. This module owns framing and validates the returned target.
pub(crate) use super::error_wire::error_reply;
use super::error_wire::reply_error;
use super::process::{StopReason, TransportError, TransportErrorKind, Worker};
use crate::computer::*;
use base64::Engine;
use serde_json::{json, Value};
use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub trait Platform: Send + Sync {
    const ID: &'static str;
    const COVERAGE: &'static str;
    const CONDITIONS: &'static [&'static str];
    const CAPABILITIES: &'static [Capability];
    const DISCOVERY_SCOPES: &'static [ObserveScopeKind] = &[ObserveScopeKind::Window];
    fn worker_args() -> Result<Vec<String>, Error>;
}
pub struct AppInfo {
    pub app_id: String,
    pub app: AppInstance,
    pub name: String,
}
pub struct NativeBackend<P: Platform> {
    worker: Arc<Worker>,
    generation: u64,
    input_policy: InputPolicy,
    cleanup: Mutex<Option<Error>>,
    #[cfg(target_os = "windows")]
    cursor: Arc<super::windows::CursorSession>,
    #[cfg(target_os = "windows")]
    cursor_operation: std::sync::atomic::AtomicU64,
    platform: PhantomData<P>,
}
pub struct NativeBinding {
    worker: Arc<Worker>,
    token: String,
}
impl Drop for NativeBinding {
    fn drop(&mut self) {
        let until = Instant::now() + Duration::from_millis(300);
        let request = json!({"request_id":uuid::Uuid::new_v4().to_string(), "operation":"release", "binding":self.token});
        let _ = self.worker.call(request.to_string().as_bytes(), || {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                Err(StopReason::TimedOut)
            } else {
                Ok(left)
            }
        });
    }
}
impl<P: Platform> NativeBackend<P> {
    pub fn spawn() -> Result<Self, Error> {
        Self::spawn_with_policy(InputPolicy::Auto)
    }
    pub fn spawn_with_policy(input_policy: InputPolicy) -> Result<Self, Error> {
        let path = std::env::current_exe().map_err(failed)?;
        let args = P::worker_args()?;
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        Ok(Self {
            worker: Arc::new(Worker::spawn_cooperative(&path, &refs).map_err(transport_error)?),
            generation: nonce(),
            input_policy,
            cleanup: Mutex::new(None),
            #[cfg(target_os = "windows")]
            cursor: Arc::new(super::windows::CursorSession::default()),
            #[cfg(target_os = "windows")]
            cursor_operation: std::sync::atomic::AtomicU64::new(0),
            platform: PhantomData,
        })
    }
    #[cfg(target_os = "windows")]
    pub fn spawn_with_cursor(
        input_policy: InputPolicy,
        cursor: Arc<super::windows::CursorSession>,
    ) -> Result<Self, Error> {
        let mut backend = Self::spawn_with_policy(input_policy)?;
        backend.cursor = cursor;
        Ok(backend)
    }
    pub fn description() -> Value {
        let value = json!({"provider":P::ID,"coverage":P::COVERAGE,"conditions":P::CONDITIONS,
            "supported_capabilities":P::CAPABILITIES.iter().map(|c|c.as_str()).collect::<Vec<_>>(),
            "supported_observe_scopes":P::DISCOVERY_SCOPES.iter().filter(|_|P::CAPABILITIES.contains(&Capability::Observe)).map(|s|s.as_str()).collect::<Vec<_>>()});
        #[cfg(target_os = "windows")]
        {
            let mut value = value;
            value["supported_window_wait_states"] =
                json!(WindowWaitState::ALL.map(WindowWaitState::as_str));
            value
        }
        #[cfg(not(target_os = "windows"))]
        value
    }
    pub fn catalog(&self, timeout: Duration) -> Result<Vec<AppInfo>, Error> {
        let until = Instant::now() + timeout;
        let poll = || {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                Err(Error::TimedOut)
            } else {
                Ok(left)
            }
        };
        let reply = self.read(json!({"operation":"catalog"}), &CallContext { poll: &poll })?;
        let entries = array(&reply, "apps")?;
        if entries.len() > 512 {
            return Err(Error::InvalidObservation);
        }
        entries
            .iter()
            .map(|v| {
                Ok(AppInfo {
                    app_id: string(v, "app_id")?.into(),
                    app: parse_app(&v["app"])?,
                    name: string(v, "name")?.into(),
                })
            })
            .collect()
    }
    #[cfg(target_os = "windows")]
    pub fn discover(&self, timeout: Duration) -> Result<Value, Error> {
        let deadline = Instant::now() + timeout;
        let poll = || {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                Err(Error::TimedOut)
            } else {
                Ok(left)
            }
        };
        let reply = self.read(
            json!({"operation":"discover_apps"}),
            &CallContext { poll: &poll },
        )?;
        if !reply["apps"].is_array() || !reply["coverage"].is_object() {
            return Err(Error::InvalidObservation);
        }
        Ok(reply)
    }
    #[cfg(target_os = "windows")]
    pub fn resolve_target(
        &self,
        app_id: &str,
        timeout: Duration,
    ) -> Result<ApplicationTarget, Error> {
        let deadline = Instant::now() + timeout;
        let poll = || {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                Err(Error::TimedOut)
            } else {
                Ok(left)
            }
        };
        let reply = self.read(
            json!({"operation":"resolve_app","app_id":app_id}),
            &CallContext { poll: &poll },
        )?;
        let apps = array(&reply, "apps")?;
        if apps.len() > 32 {
            return Err(Error::InvalidObservation);
        }
        Ok(ApplicationTarget {
            app_id: string(&reply, "app_id")?.into(),
            name: string(&reply, "name")?.into(),
            instances: apps
                .iter()
                .map(|row| parse_app(&row["app"]))
                .collect::<Result<_, _>>()?,
        })
    }
    fn rpc(&self, mut request: Value, call: &CallContext<'_>) -> Result<Value, (Error, bool)> {
        if let Some(error) = self.cleanup.lock().unwrap().clone() {
            return Err((error, false));
        }
        let id = uuid::Uuid::new_v4().to_string();
        request["request_id"] = json!(id);
        let left = call.remaining().map_err(|e| (e, false))?;
        request["deadline_unix_ms"] = json!((SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| (failed(e), false))?
            + left)
            .as_millis() as u64);
        let cause = std::sync::Mutex::new(None);
        let bytes = self
            .worker
            .call(request.to_string().as_bytes(), || match call.remaining() {
                Ok(left) if !left.is_zero() => Ok(left),
                Ok(_) => Err(StopReason::TimedOut),
                Err(error) => {
                    *cause.lock().unwrap() = Some(error);
                    Err(StopReason::Cancelled)
                }
            })
            .map_err(|error| {
                let sent = error.request_may_have_been_sent;
                (
                    cause
                        .lock()
                        .unwrap()
                        .take()
                        .or_else(|| call.remaining().err())
                        .unwrap_or_else(|| transport_error(error)),
                    sent,
                )
            })?;
        let parsed = serde_json::from_slice::<Value>(&bytes)
            .map_err(failed)
            .and_then(|reply| {
                if reply["request_id"].as_str() != Some(id.as_str()) || !reply["result"].is_object()
                {
                    Err(Error::InvalidObservation)
                } else {
                    Ok(reply["result"].clone())
                }
            });
        if let Ok(reply) = &parsed {
            if let Some(error) = reply.get("shutdown_error") {
                if error.is_object() {
                    let code = error["code"].as_str().unwrap_or("shutdown_failed");
                    let message = error["message"]
                        .as_str()
                        .unwrap_or("Native cleanup is unconfirmed");
                    self.cleanup
                        .lock()
                        .unwrap()
                        .get_or_insert(Error::NativeFailure {
                            code: code.into(),
                            message: message.into(),
                        });
                } else {
                    self.cleanup
                        .lock()
                        .unwrap()
                        .get_or_insert(Error::InvalidObservation);
                }
            }
        }
        parsed.map_err(|error| {
            let _ = self.worker.stop();
            (error, true)
        })
    }
    fn read(&self, request: Value, call: &CallContext<'_>) -> Result<Value, Error> {
        let reply = self.rpc(request, call).map_err(|e| e.0)?;
        if let Some(error) = reply_error(&reply) {
            Err(error)
        } else {
            Ok(reply)
        }
    }
    fn action(&self, request: Value, call: &CallContext<'_>) -> ActionOutcome {
        let operation = request["operation"].as_str().unwrap_or("").to_owned();
        if let Some(rejected) = policy_rejection(self.input_policy, &operation) {
            return rejected;
        }
        #[cfg(target_os = "windows")]
        let (request, mut presentation) = {
            let mut request = request;
            request["input_policy"] = json!(self.input_policy.as_str());
            let presentation = if matches!(operation.as_str(), "click" | "scroll" | "move" | "drag")
            {
                // Validate the native target before starting optional visuals.
                // The eventual action repeats every ordinary admission check.
                self.read(
                    json!({"operation":"prepare_cursor","target":request["target"]}),
                    call,
                )
                .ok()
                .and_then(|reply| self.cursor.begin(reply["cursor_target"].clone(), call))
            } else {
                None
            };
            if let Some(action) = presentation.as_ref() {
                self.cursor_operation
                    .store(action.operation, std::sync::atomic::Ordering::Release);
                request["cursor_feedback"] = action.endpoint();
            }
            (request, presentation)
        };
        let outcome = match self.rpc(request, call) {
            Err((error, true)) => ActionOutcome::unknown(error),
            Err((error, false)) => ActionOutcome::rejected(error),
            Ok(reply) => {
                let error = reply_error(&reply);
                let mut outcome = match reply["dispatch"].as_str() {
                    Some("dispatched") => ActionOutcome {
                        delivery: None,
                        dispatch: DispatchState::Dispatched,
                        evidence: EffectEvidence::Unverified,
                        error,
                    },
                    Some("not_dispatched") if error.is_some() => {
                        ActionOutcome::rejected(error.unwrap())
                    }
                    Some("unknown") => ActionOutcome::unknown(
                        error.unwrap_or_else(|| failed("Native action outcome is unknown")),
                    ),
                    _ => {
                        let _ = self.worker.stop();
                        return ActionOutcome::unknown(Error::InvalidObservation);
                    }
                };
                // Route evidence comes from native admission. Transport
                // uncertainty alone cannot establish which route was selected.
                if outcome.dispatch != DispatchState::NotDispatched {
                    #[cfg(target_os = "windows")]
                    let route = match reply["input_route"].as_str() {
                        Some("background") => Some(InputRoute::Background),
                        Some("foreground") => Some(InputRoute::Foreground),
                        _ if matches!(operation.as_str(), "click" | "scroll") => None,
                        _ => operation_route(&operation),
                    };
                    #[cfg(not(target_os = "windows"))]
                    let route = operation_route(&operation);
                    outcome.delivery = route.map(|route| InputDelivery {
                        route: Some(route),
                        reason: None,
                        foreground_borrowed: false,
                        focus_return: None,
                    });
                }
                outcome
            }
        };
        #[cfg(target_os = "windows")]
        if outcome.dispatch == DispatchState::Dispatched {
            if let Some(action) = presentation.as_mut() {
                action.keep();
            }
        }
        outcome
    }
    fn observation(
        &self,
        target: &WindowTarget,
        reply: Value,
    ) -> Result<BackendObservation<NativeBinding>, Error> {
        let observed = parse_target(&reply["target"])?;
        if &observed != target {
            return Err(Error::TargetChanged);
        }
        let token = string(&reply, "binding")?.to_owned();
        let elements = parse_elements(&reply)?;
        let image = if reply["image"].is_null() {
            None
        } else {
            let image = &reply["image"];
            let width = number(image, "width")? as u32;
            let height = number(image, "height")? as u32;
            if width == 0 || height == 0 || width > 8192 || height > 8192 {
                return Err(Error::InvalidObservation);
            }
            let png = base64::engine::general_purpose::STANDARD
                .decode(string(image, "png")?)
                .map_err(failed)?;
            if png.len() > 16 * 1024 * 1024 || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(Error::InvalidObservation);
            }
            Some(ImageFrame { width, height, png })
        };
        let discovery = if reply["discovery"].is_null() {
            None
        } else {
            Some(parse_page(&reply["discovery"])?)
        };
        Ok(BackendObservation {
            target: observed,
            elements,
            image,
            binding: NativeBinding {
                worker: self.worker.clone(),
                token,
            },
            discovery,
        })
    }
}
impl<P: Platform> Backend for NativeBackend<P> {
    type Binding = NativeBinding;
    fn is_available(&self) -> bool {
        !self.worker.is_stopped()
    }
    fn stamp(&self) -> ProviderStamp {
        ProviderStamp {
            id: P::ID.into(),
            generation: self.generation,
        }
    }
    fn supports(&self, capability: Capability) -> bool {
        P::CAPABILITIES.contains(&capability)
    }
    fn coverage(&self) -> &'static str {
        P::COVERAGE
    }
    fn conditions(&self) -> &'static [&'static str] {
        P::CONDITIONS
    }
    fn cleanup_error(&self) -> Option<Error> {
        self.cleanup
            .lock()
            .unwrap()
            .clone()
            .or_else(|| self.worker.cleanup_error().map(transport_error))
    }
    fn shutdown(&self) -> Result<(), Error> {
        let stopped = self.worker.stop().map_err(transport_error);
        #[cfg(target_os = "windows")]
        self.cursor.hide(
            self.cursor_operation
                .swap(0, std::sync::atomic::Ordering::AcqRel),
        );
        stopped?;
        self.cleanup.lock().unwrap().clone().map_or(Ok(()), Err)
    }
    #[cfg(target_os = "windows")]
    fn launch_application(&self, app_id: &str, call: &CallContext<'_>) -> ApplicationLaunch {
        let reply = match self.rpc(json!({"operation":"launch_app","app_id":app_id}), call) {
            Ok(reply) => reply,
            Err((error, sent)) => {
                return ApplicationLaunch {
                    outcome: if sent {
                        ActionOutcome::unknown(error)
                    } else {
                        ActionOutcome::rejected(error)
                    },
                    instances: vec![],
                }
            }
        };
        let error = reply_error(&reply);
        let mut outcome = match reply["dispatch"].as_str() {
            Some("not_dispatched")
                if error.is_none()
                    && reply["evidence"] == "confirmed"
                    && reply["postcondition"] == "application_running" =>
            {
                ActionOutcome {
                    delivery: None,
                    dispatch: DispatchState::NotDispatched,
                    evidence: EffectEvidence::Confirmed("application_running".into()),
                    error: None,
                }
            }
            Some("not_dispatched") if error.is_some() => ActionOutcome::rejected(error.unwrap()),
            Some("dispatched") => ActionOutcome {
                delivery: None,
                dispatch: DispatchState::Dispatched,
                evidence: if error.is_none()
                    && reply["evidence"] == "confirmed"
                    && reply["postcondition"] == "application_running"
                {
                    EffectEvidence::Confirmed("application_running".into())
                } else {
                    EffectEvidence::Unverified
                },
                error,
            },
            _ => ActionOutcome::unknown(error.unwrap_or(Error::InvalidObservation)),
        };
        let instances = match array(&reply, "apps").and_then(|apps| {
            if apps.len() > 32 {
                return Err(Error::InvalidObservation);
            }
            apps.iter()
                .map(|row| parse_app(&row["app"]))
                .collect::<Result<Vec<_>, _>>()
        }) {
            Ok(apps) => apps,
            Err(error) if outcome.dispatch != DispatchState::NotDispatched => {
                outcome = ActionOutcome::unknown(error);
                vec![]
            }
            Err(_) => vec![],
        };
        if instances.is_empty() && matches!(outcome.evidence, EffectEvidence::Confirmed(_)) {
            outcome = ActionOutcome::unknown(Error::InvalidObservation);
        }
        ApplicationLaunch { outcome, instances }
    }
    #[cfg(target_os = "windows")]
    fn quit_application(&self, app: &AppInstance, call: &CallContext<'_>) -> ActionOutcome {
        match self.rpc(
            json!({"operation":"quit_app","target":{"app":app_json(app)}}),
            call,
        ) {
            Err((error, true)) => ActionOutcome::unknown(error),
            Err((error, false)) => ActionOutcome::rejected(error),
            Ok(reply) => {
                let error = reply_error(&reply);
                match reply["dispatch"].as_str() {
                    Some("not_dispatched") if error.is_some() => {
                        ActionOutcome::rejected(error.unwrap())
                    }
                    Some("dispatched") => ActionOutcome {
                        delivery: None,
                        dispatch: DispatchState::Dispatched,
                        evidence: if error.is_none()
                            && reply["evidence"] == "confirmed"
                            && reply["postcondition"] == "application_exited"
                        {
                            EffectEvidence::Confirmed("application_exited".into())
                        } else {
                            EffectEvidence::Unverified
                        },
                        error,
                    },
                    _ => ActionOutcome::unknown(error.unwrap_or(Error::InvalidObservation)),
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    fn close_window(&self, target: &WindowTarget, call: &CallContext<'_>) -> ActionOutcome {
        match self.rpc(
            json!({"operation":"close_window","target":target_json(target)}),
            call,
        ) {
            Err((error, true)) => ActionOutcome::unknown(error),
            Err((error, false)) => ActionOutcome::rejected(error),
            Ok(reply) => {
                let error = reply_error(&reply);
                match reply["dispatch"].as_str() {
                    Some("not_dispatched") if error.is_some() => {
                        ActionOutcome::rejected(error.unwrap())
                    }
                    Some("dispatched") => ActionOutcome {
                        delivery: None,
                        dispatch: DispatchState::Dispatched,
                        evidence: if error.is_none()
                            && reply["evidence"] == "confirmed"
                            && reply["postcondition"] == "window_closed"
                        {
                            EffectEvidence::Confirmed("window_closed".into())
                        } else {
                            EffectEvidence::Unverified
                        },
                        error,
                    },
                    _ => ActionOutcome::unknown(error.unwrap_or(Error::InvalidObservation)),
                }
            }
        }
    }
    fn windows(&self, app: &AppInstance, call: &CallContext<'_>) -> Result<WindowInventory, Error> {
        let reply = self.read(json!({"operation":"windows","app":app_json(app)}), call)?;
        let values = array(&reply, "windows")?;
        if values.len() > 256 {
            return Err(Error::InvalidObservation);
        }
        let windows = values
            .iter()
            .map(|v| {
                let target = parse_target(&v["target"])?;
                if &target.app != app {
                    return Err(Error::WrongApp);
                }
                Ok(WindowInfo {
                    scene: parse_layout(v.get("scene"))?,
                    target,
                    title: string(v, "title")?.into(),
                    visible: v["visible"].as_bool().ok_or(Error::InvalidObservation)?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(WindowInventory {
            report: InventoryReport {
                scope: InventoryScope::SameProcessStandardWindows,
                candidate_count: windows.len() as u32,
                supported_count: windows.len() as u32,
                excluded_count: 0,
                diagnostics: InventoryDiagnostics {
                    provider: P::ID.into(),
                    counters: vec![],
                    error_codes: vec![],
                },
            },
            windows,
        })
    }
    #[cfg(target_os = "windows")]
    fn supported_window_wait_states(&self) -> &'static [WindowWaitState] {
        &WindowWaitState::ALL
    }
    #[cfg(target_os = "windows")]
    fn probe_window_state(
        &self,
        target: &WindowTarget,
        state: WindowWaitState,
        call: &CallContext<'_>,
    ) -> Result<WindowStateProbe, Error> {
        let reply=self.read(json!({"operation":"probe_window_state","target":target_json(target),"state":state.as_str()}),call)?;
        super::windows::window_state::parse(&reply, &target.app)
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        self.observe_page(target, &ObserveQuery::default(), call)
    }
    fn observe_page(
        &self,
        target: &WindowTarget,
        query: &ObserveQuery,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        self.observe_scoped(target, query, ObserveScope::Window, None, call)
    }
    fn supports_observe_scope(&self, scope: ObserveScopeKind) -> bool {
        P::DISCOVERY_SCOPES.contains(&scope)
    }
    fn observe_scoped(
        &self,
        target: &WindowTarget,
        query: &ObserveQuery,
        scope: ObserveScope,
        binding: Option<&Self::Binding>,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        if !self.supports_observe_scope(scope.kind()) {
            return Err(Error::UnsupportedCapability);
        }
        let scope = match scope {
            ObserveScope::Window => json!({"kind":"window"}),
            ObserveScope::Menu => json!({"kind":"menu"}),
            ObserveScope::Subtree(element) => {
                json!({"kind":"subtree","binding":binding.ok_or(Error::InvalidObservation)?.token,"element":element.0})
            }
            ObserveScope::Region(rect) => {
                json!({"kind":"region","binding":binding.ok_or(Error::InvalidObservation)?.token,"rect":[rect.x,rect.y,rect.width,rect.height]})
            }
        };
        let reply=self.read(json!({"operation":"observe","target":target_json(target),"query":{"role":query.role,"text":query.text,"limit":query.limit,"scope":scope}}),call)?;
        self.observation(target, reply)
    }
    fn read_text(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        request: TextRequest,
        call: &CallContext<'_>,
    ) -> Result<BackendTextPage, Error> {
        let reply = self.read(
            json!({"operation":"read_text","target":target_json(target),"binding":binding.token,
            "element":request.element.0,"offset":request.offset,"limit":request.limit}),
            call,
        )?;
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
        binding: &Self::Binding,
        call: &CallContext<'_>,
    ) -> Result<BackendObservationPage, Error> {
        let reply=self.read(json!({"operation":"observe_next","target":target_json(target),"binding":binding.token}),call)?;
        let observed = parse_target(&reply["target"])?;
        if &observed != target {
            return Err(Error::TargetChanged);
        }
        Ok(BackendObservationPage {
            target: observed,
            elements: parse_elements(&reply)?,
            discovery: parse_page(&reply["discovery"])?,
        })
    }
    fn capture(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        let reply = self.read(
            json!({"operation":"capture","target":target_json(target)}),
            call,
        )?;
        self.observation(target, reply)
    }
    fn press(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.action(json!({"operation":"press","target":target_json(target),"binding":binding.token,"element":element.0}),call)
    }
    fn set_value(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        element: ElementId,
        value: &str,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.action(json!({"operation":"setValue","target":target_json(target),"binding":binding.token,"element":element.0,"value":value}),call)
    }
    fn click(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        point: PixelPoint,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.click_with_options(target, binding, point, ClickOptions::default(), call)
    }
    fn click_with_options(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        point: PixelPoint,
        options: ClickOptions,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.action(json!({"operation":"click","target":target_json(target),"binding":binding.token,"x":point.x,"y":point.y,"button":if options.button==MouseButton::Left{"left"}else{"right"},"count":options.count}),call)
    }
    #[cfg(target_os = "windows")]
    fn pointer_gesture(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        gesture: &PointerGesture,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        let path = gesture
            .points()
            .iter()
            .map(|point| json!({"x":point.x,"y":point.y}))
            .collect::<Vec<_>>();
        self.action(
            json!({"operation":match gesture { PointerGesture::Move(_) => "move", PointerGesture::Drag(_) => "drag" },
                "target":target_json(target),"binding":binding.token,"path":path}),
            call,
        )
    }
    fn input(
        &self,
        target: &WindowTarget,
        input: &WindowInput,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        let mut request = match input {
            WindowInput::Focus => json!({"operation":"focus"}),
            WindowInput::TypeText { text } => json!({"operation":"typeText","text":text}),
            WindowInput::Keypress { keys } => json!({"operation":"keypress","keys":keys}),
            WindowInput::Scroll { delta_x, delta_y } => {
                json!({"operation":"scroll","delta_x":delta_x,"delta_y":delta_y})
            }
        };
        request["target"] = target_json(target);
        self.action(request, call)
    }
    #[cfg(target_os = "windows")]
    fn supports_scroll_anchor(&self, kind: ObservationKind) -> bool {
        matches!(kind, ObservationKind::Pixels | ObservationKind::Semantic)
    }
    #[cfg(target_os = "windows")]
    fn scroll_at(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        scroll: ScrollRequest,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        let anchor = match scroll.anchor {
            ScrollAnchor::Pixels(point) => json!({"kind":"pixels","x":point.x,"y":point.y}),
            ScrollAnchor::Element(element) => json!({"kind":"element","element":element.0}),
        };
        self.action(
            json!({"operation":"scroll","target":target_json(target),"binding":binding.token,
            "anchor":anchor,"delta_x":scroll.delta_x,"delta_y":scroll.delta_y}),
            call,
        )
    }
}
fn operation_route(operation: &str) -> Option<InputRoute> {
    match operation {
        // Windows uses UIA Invoke/Toggle/Select and SetValue without an explicit
        // foreground operation. X11 currently does not advertise these actions.
        "press" | "setValue" => Some(InputRoute::Semantic),
        "click" | "focus" | "typeText" | "keypress" | "scroll" | "move" | "drag" => {
            Some(InputRoute::Foreground)
        }
        _ => None,
    }
}

fn policy_rejection(policy: InputPolicy, operation: &str) -> Option<ActionOutcome> {
    // Windows selects a targeted HWND route after proving the live receiver.
    // Keyboard/focus and other platforms retain their existing policy gate.
    #[cfg(target_os = "windows")]
    if matches!(operation, "click" | "scroll") {
        return None;
    }
    if policy != InputPolicy::BackgroundOnly
        || operation_route(operation) != Some(InputRoute::Foreground)
    {
        return None;
    }
    let mut outcome = ActionOutcome::rejected(Error::NativeFailure {
        code: "foreground_required".into(),
        message:
            "This platform requires foreground input; the service allows background operations only"
                .into(),
    });
    outcome.delivery = Some(InputDelivery {
        route: None,
        reason: Some("platform_has_no_background_route".into()),
        foreground_borrowed: false,
        focus_return: None,
    });
    Some(outcome)
}

pub fn nonce() -> u64 {
    u64::from_ne_bytes(uuid::Uuid::new_v4().as_bytes()[..8].try_into().unwrap()) | 1
}
pub fn failed(error: impl std::fmt::Display) -> Error {
    Error::BackendFailed(error.to_string())
}
fn transport_error(error: TransportError) -> Error {
    match error.kind {
        TransportErrorKind::Control(StopReason::Cancelled) => Error::Cancelled,
        TransportErrorKind::Control(StopReason::TimedOut) => Error::TimedOut,
        _ => failed(error),
    }
}
pub fn app_json(app: &AppInstance) -> Value {
    json!({"identity":app.identity.0,"pid":app.pid,"generation":app.generation})
}
pub fn target_json(target: &WindowTarget) -> Value {
    json!({"app":app_json(&target.app),"native_id":target.native_id,"generation":target.generation})
}
pub fn parse_app(v: &Value) -> Result<AppInstance, Error> {
    Ok(AppInstance {
        identity: AppIdentity(string(v, "identity")?.into()),
        pid: u32::try_from(number(v, "pid")?).map_err(failed)?,
        generation: number(v, "generation")?,
    })
}
/// Optional layout is a provider boundary; malformed facts never become
/// fabricated image coordinates.
pub(crate) fn parse_layout(value: Option<&Value>) -> Result<Option<WindowLayout>, Error> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let raw = value["bounds"]
        .as_array()
        .filter(|b| b.len() == 4)
        .ok_or(Error::InvalidObservation)?;
    let mut bounds = [0.0; 4];
    for (i, component) in raw.iter().enumerate() {
        bounds[i] = component.as_f64().ok_or(Error::InvalidObservation)?;
    }
    let rank = value["rank"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(Error::InvalidObservation)?;
    let mut layout = WindowLayout::new(bounds, rank).ok_or(Error::InvalidObservation)?;
    layout.keyboard_focused = value
        .get("keyboard_focused")
        .map(|v| v.as_bool().ok_or(Error::InvalidObservation))
        .transpose()?;
    layout.keyboard_modal = value
        .get("keyboard_modal")
        .map(|v| v.as_bool().ok_or(Error::InvalidObservation))
        .transpose()?;
    layout.semantic_candidate = value
        .get("semantic_candidate")
        .map(|v| v.as_bool().ok_or(Error::InvalidObservation))
        .transpose()?;
    Ok(Some(layout))
}

pub fn parse_target(v: &Value) -> Result<WindowTarget, Error> {
    Ok(WindowTarget {
        app: parse_app(&v["app"])?,
        native_id: number(v, "native_id")?,
        generation: number(v, "generation")?,
    })
}
pub fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str, Error> {
    v[key].as_str().ok_or(Error::InvalidObservation)
}
pub fn number(v: &Value, key: &str) -> Result<u64, Error> {
    v[key].as_u64().ok_or(Error::InvalidObservation)
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, Error> {
    v[key].as_array().ok_or(Error::InvalidObservation)
}
fn parse_page(v: &Value) -> Result<DiscoveryPage, Error> {
    let page = DiscoveryPage {
        has_more: v["has_more"].as_bool().ok_or(Error::InvalidObservation)?,
        complete: v["complete"].as_bool().ok_or(Error::InvalidObservation)?,
        reason: v["reason"].as_str().map(str::to_owned),
        visited: u32::try_from(number(v, "visited")?).map_err(failed)?,
    };
    if page.visited > 8192 || (page.has_more && page.complete) {
        return Err(Error::InvalidObservation);
    }
    Ok(page)
}
fn parse_elements(v: &Value) -> Result<Vec<Element>, Error> {
    array(v, "elements")?
        .iter()
        .map(|v| {
            let actions = array(v, "actions")?
                .iter()
                .map(|a| {
                    a.as_str()
                        .map(str::to_owned)
                        .ok_or(Error::InvalidObservation)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Element {
                id: ElementId(number(v, "id")?),
                rebind_identity: match v.get("rebind_identity") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(id)) if !id.is_empty() && id.len() <= 256 => {
                        Some(id.clone())
                    }
                    _ => return Err(Error::InvalidObservation),
                },
                identity: match v.get("identity") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(id)) if !id.is_empty() && id.len() <= 256 => {
                        Some(id.clone())
                    }
                    _ => return Err(Error::InvalidObservation),
                },
                label: string(v, "label")?.into(),
                role: string(v, "role")?.into(),
                name: v["name"].as_str().map(str::to_owned),
                value: v["value"].as_str().map(str::to_owned),
                enabled: v["enabled"].as_bool(),
                can_press: actions.iter().any(|a| a == "press"),
                actions,
            })
        })
        .collect()
}
pub fn deadline(request: &Value) -> Result<Instant, Error> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(failed)?
        .as_millis() as u64;
    let left = number(request, "deadline_unix_ms")?
        .saturating_sub(now)
        .min(30_000);
    if left == 0 {
        Err(Error::TimedOut)
    } else {
        Ok(Instant::now() + Duration::from_millis(left))
    }
}
pub fn check(until: Instant) -> Result<(), Error> {
    if Instant::now() >= until {
        Err(Error::TimedOut)
    } else {
        Ok(())
    }
}
pub fn encode_png(width: u32, height: u32, rgb: &[u8]) -> Result<Value, Error> {
    encode_png_color(width, height, rgb, png::ColorType::Rgb)
}
pub fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Value, Error> {
    encode_png_color(width, height, rgba, png::ColorType::Rgba)
}
fn encode_png_color(
    width: u32,
    height: u32,
    pixels: &[u8],
    color: png::ColorType,
) -> Result<Value, Error> {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(failed)?;
        writer.write_image_data(pixels).map_err(failed)?;
    }
    if png.len() > 16 * 1024 * 1024 {
        return Err(failed("PNG exceeds 16 MiB"));
    }
    Ok(
        json!({"width":width,"height":height,"png":base64::engine::general_purpose::STANDARD.encode(png)}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_metadata_is_optional_but_never_invented_or_partially_accepted() {
        assert!(parse_layout(None).unwrap().is_none());
        let layout = parse_layout(Some(&json!({"bounds":[-100,20,300,200],"rank":2})))
            .unwrap()
            .unwrap();
        assert_eq!(layout.bounds(), [-100.0, 20.0, 300.0, 200.0]);
        assert_eq!(layout.rank, 2);
        assert_eq!(layout.keyboard_focused, None);
        assert_eq!(layout.semantic_candidate, None);
        for candidate in [true, false] {
            let hint = json!({"bounds":[-100,20,300,200],"rank":2,"semantic_candidate":candidate});
            assert_eq!(
                parse_layout(Some(&hint))
                    .unwrap()
                    .unwrap()
                    .semantic_candidate,
                Some(candidate)
            );
        }
        for invalid in [Value::Null, json!("false"), json!(0), json!({}), json!([])] {
            let hint = json!({"bounds":[-100,20,300,200],"rank":2,"semantic_candidate":invalid});
            assert!(parse_layout(Some(&hint)).is_err());
        }
        let hint = json!({"bounds":[-100,20,300,200],"rank":2,"keyboard_focused":true});
        assert_eq!(
            parse_layout(Some(&hint)).unwrap().unwrap().keyboard_focused,
            Some(true)
        );
        let modal = json!({"bounds":[-100,20,300,200],"rank":2,"keyboard_modal":true});
        assert_eq!(
            parse_layout(Some(&modal)).unwrap().unwrap().keyboard_modal,
            Some(true)
        );
        let bad_modal = json!({"bounds":[-100,20,300,200],"rank":2,"keyboard_modal":"true"});
        assert!(parse_layout(Some(&bad_modal)).is_err());
        let invalid_hint = json!({"bounds":[-100,20,300,200],"rank":2,"keyboard_focused":"yes"});
        assert!(parse_layout(Some(&invalid_hint)).is_err());
        for value in [
            json!({"bounds":[0,0,0,20],"rank":0}),
            json!({"bounds":[0,0,20],"rank":0}),
            json!({"bounds":[0,0,20,20],"rank":-1}),
            json!({"bounds":[0,0,20,20],"rank":4294967296u64}),
        ] {
            assert!(parse_layout(Some(&value)).is_err(), "{value}");
        }
    }

    #[test]
    fn text_range_and_unsupported_native_errors_keep_their_core_identity() {
        for (error, code) in [
            (Error::UnsupportedCapability, "unsupported_capability"),
            (Error::StaleObservation, "stale_observation"),
        ] {
            let reply = error_reply(error.clone(), false);
            assert_eq!(reply["error"], code);
            assert_eq!(reply_error(&reply), Some(error));
            assert!(reply.get("dispatch").is_none());
        }
        let native_range = json!({"error":"invalid_text_range","message":"Use a UTF-16 boundary"});
        let range = reply_error(&native_range).unwrap();
        assert_eq!(
            range,
            Error::NativeFailure {
                code: "invalid_text_range".into(),
                message: "Use a UTF-16 boundary".into(),
            }
        );
        let encoded = error_reply(range, false);
        assert_eq!(encoded["error"], "invalid_text_range");
        assert!(
            matches!(reply_error(&encoded), Some(Error::NativeFailure { code, .. }) if code == "invalid_text_range")
        );
    }

    #[test]
    fn background_only_rejects_foreground_paths_and_preserves_semantics() {
        for operation in ["click", "focus", "typeText", "keypress", "scroll"] {
            #[cfg(target_os = "windows")]
            if matches!(operation, "click" | "scroll") {
                assert!(policy_rejection(InputPolicy::BackgroundOnly, operation).is_none());
                continue;
            }
            let rejected = policy_rejection(InputPolicy::BackgroundOnly, operation).unwrap();
            assert_eq!(rejected.dispatch, DispatchState::NotDispatched);
            assert_eq!(
                rejected.delivery,
                Some(InputDelivery {
                    route: None,
                    reason: Some("platform_has_no_background_route".into()),
                    foreground_borrowed: false,
                    focus_return: None,
                })
            );
            assert!(
                matches!(rejected.error, Some(Error::NativeFailure { code, .. }) if code == "foreground_required")
            );
            assert!(policy_rejection(InputPolicy::Auto, operation).is_none());
        }
        for operation in ["press", "setValue"] {
            assert!(policy_rejection(InputPolicy::BackgroundOnly, operation).is_none());
            assert_eq!(operation_route(operation), Some(InputRoute::Semantic));
        }
    }

    #[test]
    fn returned_target_retains_both_process_and_window_incarnations() {
        let target = WindowTarget {
            app: AppInstance {
                identity: AppIdentity("app.exe".into()),
                pid: 10,
                generation: 20,
            },
            native_id: 30,
            generation: 40,
        };
        assert_eq!(parse_target(&target_json(&target)).unwrap(), target);
        let mut altered = target_json(&target);
        altered["app"]["generation"] = json!(21);
        assert_ne!(parse_target(&altered).unwrap(), target);
    }
    #[test]
    fn impossible_discovery_status_is_not_accepted() {
        assert!(
            parse_page(&json!({"has_more":true,"complete":true,"visited":1,"reason":null}))
                .is_err()
        );
        assert!(parse_page(
            &json!({"has_more":false,"complete":false,"visited":8193,"reason":"scan_limit"})
        )
        .is_err());
        assert!(
            !parse_page(
                &json!({"has_more":false,"complete":false,"visited":8192,"reason":"scan_limit"})
            )
            .unwrap()
            .complete
        );
    }
}

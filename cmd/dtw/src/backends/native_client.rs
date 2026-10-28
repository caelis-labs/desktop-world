//! Bounded DTW protocol shared by the Windows and experimental X11 adapters.
//! Native objects never leave their dedicated process; an interrupted call is
//! never replayed. This module owns framing and validates the returned target.
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
    cleanup: Mutex<Option<Error>>,
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
        let path = std::env::current_exe().map_err(failed)?;
        let args = P::worker_args()?;
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        Ok(Self {
            worker: Arc::new(Worker::spawn_cooperative(&path, &refs).map_err(transport_error)?),
            generation: nonce(),
            cleanup: Mutex::new(None),
            platform: PhantomData,
        })
    }
    pub fn description() -> Value {
        json!({"provider":P::ID,"coverage":P::COVERAGE,"conditions":P::CONDITIONS,
            "supported_capabilities":P::CAPABILITIES.iter().map(|c|c.as_str()).collect::<Vec<_>>()})
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
        match self.rpc(request, call) {
            Err((error, true)) => ActionOutcome::unknown(error),
            Err((error, false)) => ActionOutcome::rejected(error),
            Ok(reply) => {
                let error = reply_error(&reply);
                match reply["dispatch"].as_str() {
                    Some("dispatched") => ActionOutcome {
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
                        ActionOutcome::unknown(Error::InvalidObservation)
                    }
                }
            }
        }
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
        self.worker.stop().map_err(transport_error)?;
        self.cleanup.lock().unwrap().clone().map_or(Ok(()), Err)
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
        let reply=self.read(json!({"operation":"observe","target":target_json(target),"query":{"role":query.role,"text":query.text,"limit":query.limit}}),call)?;
        self.observation(target, reply)
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
fn reply_error(v: &Value) -> Option<Error> {
    let code = v["error"].as_str()?;
    Some(match code {
        "target_changed" => Error::TargetChanged,
        "stale_observation" => Error::StaleObservation,
        "timed_out" => Error::TimedOut,
        "unsupported_capability" => Error::UnsupportedCapability,
        "busy" => Error::Busy,
        "invalid_point" => Error::InvalidPoint,
        _ => Error::NativeFailure {
            code: code.into(),
            message: v["message"].as_str().unwrap_or(code).into(),
        },
    })
}
pub fn error_reply(error: Error, action: bool) -> Value {
    let code = match &error {
        Error::TargetChanged | Error::WrongApp => "target_changed",
        Error::StaleObservation => "stale_observation",
        Error::TimedOut => "timed_out",
        Error::UnsupportedCapability => "unsupported_capability",
        Error::Busy => "busy",
        Error::InvalidPoint => "invalid_point",
        Error::NativeFailure { code, .. } => code,
        _ => "native_failure",
    };
    let mut reply = json!({"error":code,"message":format!("{error:?}")});
    if action {
        reply["dispatch"] = json!("not_dispatched");
    }
    reply
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
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(failed)?;
        writer.write_image_data(rgb).map_err(failed)?;
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

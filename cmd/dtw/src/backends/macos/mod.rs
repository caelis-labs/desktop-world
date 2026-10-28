//! Narrow macOS provider. All native objects and SCK clients belong to its worker.
use super::process::{StopReason, TransportError, TransportErrorKind, Worker};
use crate::computer::*;
use base64::Engine;
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
mod worker;
pub use worker::worker_entry;

pub struct AppInfo {
    pub app: AppInstance,
    pub name: String,
}
pub struct NativeBackend {
    worker: Arc<Worker>,
    stamp: ProviderStamp,
}
pub struct NativeBinding {
    worker: Arc<Worker>,
    semantic: Option<String>,
    pixels: Option<(Value, u32, u32)>,
}
impl Drop for NativeBinding {
    fn drop(&mut self) {
        if let Some(token) = self.semantic.take() {
            // Cleanup is authority-free: it only releases objects. It cannot
            // extend a grant or invoke input, and a failed worker is never revived.
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
impl NativeBackend {
    pub fn spawn() -> Result<Self, Error> {
        let exe = std::env::current_exe().map_err(failed)?;
        let worker = Worker::spawn(&exe, &["--dtw-native-worker"]).map_err(transport_error)?;
        Ok(Self {
            worker: Arc::new(worker),
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
                    app: parse_app(&entry["app"])?,
                    name: string(entry, "name")?.into(),
                })
            })
            .collect()
    }
    fn rpc(&self, mut request: Value, call: &CallContext<'_>) -> Result<Value, (Error, bool)> {
        let request_id = unique_generation();
        request["request_id"] = json!(request_id);
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
        let bytes = self
            .worker
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
            serde_json::from_slice(&bytes).map_err(|e| (self.retire(failed(e)), true))?;
        envelope_reply(&reply, request_id).map_err(|error| (self.retire(error), true))
    }

    fn retire(&self, error: Error) -> Error {
        match self.worker.stop() {
            Ok(()) => error,
            Err(shutdown) => Error::BackendFailed(format!(
                "{error:?}; native worker shutdown failed: {shutdown}"
            )),
        }
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
        let reply = match self.rpc(request, call) {
            Ok(reply) => reply,
            Err((error, true)) => return ActionOutcome::unknown(error),
            Err((error, false)) => return ActionOutcome::rejected(error),
        };
        let mut outcome = action_reply(&reply);
        if outcome.error == Some(Error::InvalidObservation) {
            outcome.error = Some(self.retire(Error::InvalidObservation));
        }
        outcome
    }
}
impl Backend for NativeBackend {
    type Binding = NativeBinding;
    fn shutdown(&self) -> Result<(), Error> {
        self.worker.stop().map_err(transport_error)
    }
    fn stamp(&self) -> ProviderStamp {
        self.stamp.clone()
    }
    fn supports(&self, _: Capability) -> bool {
        true
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
        let reply = self.read_rpc(
            json!({"operation":"observe","target":target_json(target)}),
            call,
        )?;
        // Construct the guard before parsing the rest so invalid replies still release it.
        let binding = NativeBinding {
            worker: self.worker.clone(),
            semantic: Some(string(&reply, "binding")?.into()),
            pixels: None,
        };
        if reply["truncated"].as_bool() != Some(false) {
            return Err(Error::BackendFailed(
                "AX snapshot incomplete at the supported node/depth limit".into(),
            ));
        }
        let elements = array(&reply, "elements")?
            .iter()
            .map(|e| {
                Ok(Element {
                    id: ElementId(number(e, "id")?),
                    label: string(e, "label")?.into(),
                    can_press: e["can_press"].as_bool().ok_or(Error::InvalidObservation)?,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(BackendObservation {
            target: parse_target(&reply["target"])?,
            elements,
            image: None,
            binding,
        })
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
        let reply = self.read_rpc(
            json!({"operation":"capture","target":target_json(target)}),
            call,
        )?;
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
        let Some((bounds, width, height)) = &binding.pixels else {
            return ActionOutcome::rejected(Error::InvalidObservation);
        };
        let screen_x =
            bounds[0].as_f64().unwrap() + point.x / *width as f64 * bounds[2].as_f64().unwrap();
        let screen_y =
            bounds[1].as_f64().unwrap() + point.y / *height as f64 * bounds[3].as_f64().unwrap();
        self.action(json!({"operation":"click","target":target_json(target),"bounds":bounds,"screen_x":screen_x,"screen_y":screen_y}),call)
    }
}
fn envelope_reply(reply: &Value, request_id: u64) -> Result<Value, Error> {
    if reply["request_id"].as_u64() != Some(request_id) || !reply["result"].is_object() {
        Err(Error::InvalidObservation)
    } else {
        Ok(reply["result"].clone())
    }
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
    ActionOutcome {
        dispatch,
        evidence: EffectEvidence::Unverified,
        error,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
            transport_cause(control, Some(Error::GrantExpired)),
            Error::GrantExpired
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
        _ => Error::BackendFailed(format!(
            "{code}: {}",
            v["message"].as_str().unwrap_or("native error")
        )),
    })
}

fn parse_inventory_report(value: &Value, proven: bool) -> Result<InventoryReport, Error> {
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

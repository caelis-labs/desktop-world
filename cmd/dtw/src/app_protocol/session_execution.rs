//! Session-only VM: no provider, catalog, window owner or desktop dispatch.
use super::*;
use crate::app_protocol::{script_engine as engine, scripts::EngineThread};
use std::sync::mpsc;

impl<F: Factory> Server<F> {
    pub(super) fn execute_session(
        &self,
        call: &Call<F::Provider>,
        id: &str,
        source: &str,
        timeout_ms: u64,
        max_calls: u32,
    ) -> Reply {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let cancel = Arc::new(AtomicBool::new(call.cancelled.load(Ordering::Acquire)));
        let initial = self.session_state.lock().unwrap().clone();
        let metadata = json!({"id":self.session_id,"app":null});
        let source = source.to_owned();
        let limits = engine::Limits {
            deadline,
            max_calls,
            memory_bytes: scripts::MEMORY_BYTES,
            stack_bytes: scripts::STACK_BYTES,
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let engine_cancel = cancel.clone();
        let handle = match std::thread::Builder::new()
            .name("dtw-session-script".into())
            .spawn(move || {
                engine::run_with_session(&source, initial, metadata, limits, engine_cancel, sender)
            }) {
            Ok(handle) => handle,
            Err(error) => {
                return problem(
                    "script_internal",
                    &format!("Cannot start session VM: {error}"),
                )
            }
        };
        let thread = EngineThread {
            handle: Some(handle),
            cancel: cancel.clone(),
        };
        let mut steps = Vec::new();
        let mut step_bytes = 0;
        let mut image = None;
        let mut capture = None;
        loop {
            if call.cancelled.load(Ordering::Acquire) {
                cancel.store(true, Ordering::Release);
            }
            match receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(host) => {
                    let reply = if step_bytes >= scripts::MAX_STEP_RECEIPT_BYTES {
                        Err(WireError {
                            code: "receipt_limit".into(),
                            message: "Session step receipt budget exhausted".into(),
                            details: None,
                        })
                    } else if host.op == "session.result" {
                        self.session_receipt("", id, &host.args)
                    } else {
                        Err(WireError {code:"app_required".into(),
                            message:"Session-only execution cannot operate applications; supply app in a new exec".into(),details:None})
                    };
                    let error = reply.as_ref().err().map(super::super::error_json);
                    let response = match reply {
                        Ok(reply) => {
                            if let Some(original) = reply.image {
                                image = Some(original);
                                capture = Some(
                                    json!({"step":host.number,"recovered_from":reply.data["execution_id"],
                                    "observation":reply.data["value"]["last_capture"],"timing":"historical"}),
                                );
                            }
                            Ok(reply.data)
                        }
                        Err(error) => Err(engine::HostError {
                            message: error.message,
                            code: Some(error.code),
                            details: error.details,
                        }),
                    };
                    let step = json!({"number":host.number,"line":host.line,"operation":host.op,
                        "status":"completed","receipt":null,"error":error,"inventory":null});
                    step_bytes += step.to_string().len();
                    steps.push(step);
                    let _ = host.reply.send(response);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let outcome = match thread.join() {
            Ok(outcome) => outcome,
            Err(_) => {
                return problem(
                    "script_internal",
                    "Session VM panicked; no desktop operation was available",
                )
            }
        };
        if let Some(capture) = &mut capture {
            capture["later_calls"] = json!(
                u64::from(outcome.calls).saturating_sub(capture["step"].as_u64().unwrap_or(0))
            );
        }
        let mut value = json!({"console":outcome.console,"console_truncated":outcome.console_truncated,
            "calls":outcome.calls,"last_capture":capture});
        let (error, failure, committed) = match outcome.result {
            Ok(success) if !call.cancelled.load(Ordering::Acquire) && Instant::now() < deadline => {
                *self.session_state.lock().unwrap() = success.state;
                value["result"] = success.value;
                (None, Value::Null, true)
            }
            Ok(_) => {
                let (kind, code) = if call.cancelled.load(Ordering::Acquire) {
                    ("cancelled", "script_cancelled")
                } else {
                    ("timeout", "script_timeout")
                };
                (
                    Some(
                        json!({"code":code,"message":"Session execution stopped before state commit"}),
                    ),
                    json!({"kind":kind}),
                    false,
                )
            }
            Err(failure) => {
                let code = match failure.kind.as_str() {
                    "syntax" => "script_syntax",
                    "cancelled" => "script_cancelled",
                    "timeout" => "script_timeout",
                    "memory" => "script_memory",
                    _ => "script_exception",
                };
                (
                    Some(json!({"code":code,"message":failure.message})),
                    json!({"kind":failure.kind,
                    "message":failure.message,"line":failure.line,"op":failure.op,"code":failure.code,"call":failure.call}),
                    false,
                )
            }
        };
        Reply {
            data: json!({"version":1,"status":"completed","session_id":self.session_id,
            "session_only":true,"value":value,"error":error,
            "script":{"steps":steps,"failure":failure,"state_committed":committed,"shutdown_error":null}}),
            image,
            is_error: error.is_some(),
        }
    }
}

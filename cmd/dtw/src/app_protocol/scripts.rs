//! App-bound scripts reuse the outer job reservation and the existing Core.
//! The step ledger is independent of evictable result/image payloads.
use super::{
    app_execution::AppCall, script_engine as engine, Command, Completed, Coordinator, Error,
    Submit, WireError, Work,
};
use crate::computer::Backend;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

pub(super) const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub(super) const DEFAULT_TIMEOUT_MS: u64 = 30_000;
// Internal wire bounds, not a workflow-sized operation quota. Public MCP
// callers supply only app/source; execution policy owns these budgets.
pub(super) const MAX_TIMEOUT_MS: u64 = u32::MAX as u64;
pub(super) const MAX_CALLS: u32 = u32::MAX;
pub(super) const UNLIMITED_CALLS: u32 = 0;
pub(super) const MEMORY_BYTES: usize = 32 * 1024 * 1024;
pub(super) const STACK_BYTES: usize = 512 * 1024;

pub(super) fn budgets(values: &serde_json::Map<String, Value>) -> Result<(u64, u32), WireError> {
    let integer = |key: &str, default: u64, maximum: u64| match values.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_f64()
            .filter(|v| v.is_finite() && v.fract() == 0.0 && *v >= 1.0 && *v <= maximum as f64)
            .map(|v| v as u64)
            .ok_or_else(|| WireError {
                code: "invalid_arguments".into(),
                details: None,
                message: format!(
                    "{key} must be a positive integer within the wire range 1..={maximum}"
                ),
            }),
    };
    Ok((
        integer("timeout_ms", DEFAULT_TIMEOUT_MS, MAX_TIMEOUT_MS)?,
        integer("max_calls", UNLIMITED_CALLS as u64, MAX_CALLS as u64)? as u32,
    ))
}

pub(super) fn description() -> Value {
    super::description::script(MEMORY_BYTES, STACK_BYTES)
}

// Dropping a JoinHandle detaches it. This owner keeps the VM inside the job
// lifetime even when a Core provider panics while the VM awaits its reply.
pub(super) struct EngineThread {
    pub(super) handle: Option<std::thread::JoinHandle<engine::Outcome>>,
    pub(super) cancel: Arc<AtomicBool>,
}
impl EngineThread {
    pub(super) fn join(mut self) -> std::thread::Result<engine::Outcome> {
        self.handle.take().unwrap().join()
    }
}
impl Drop for EngineThread {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.cancel.store(true, Ordering::Release);
            let _ = handle.join();
        }
    }
}

impl<B: Backend + 'static> Coordinator<B> {
    pub(super) fn execute_script(&self, submitted: &Submit) -> Work {
        let Command::Script {
            source, max_calls, ..
        } = &submitted.command
        else {
            unreachable!()
        };
        let id = &submitted.request_id;
        let (deadline, cancel, initial) = {
            let state = self.state.lock().unwrap();
            let run = state
                .jobs
                .iter()
                .find(|j| j.submitted.request_id == *id)
                .unwrap()
                .script
                .as_ref()
                .unwrap();
            let initial = state.script_state.lock().unwrap().clone();
            (run.deadline, run.cancel.clone(), initial)
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let source = source.clone();
        let limits = engine::Limits {
            deadline,
            max_calls: *max_calls,
            memory_bytes: MEMORY_BYTES,
            stack_bytes: STACK_BYTES,
        };
        let session = self.session_metadata();
        let agent_surface = session["agent"] == true;
        let engine_cancel = cancel.clone();
        let thread = match std::thread::Builder::new()
            .name("dtw-app-script".into())
            .spawn(move || {
                engine::run_with_session(&source, initial, session, limits, engine_cancel, sender)
            }) {
            Ok(thread) => thread,
            Err(error) => {
                return Work::read_error(Error::BackendFailed(format!(
                    "Cannot start script engine: {error}"
                )))
            }
        };
        let thread = EngineThread {
            handle: Some(thread),
            cancel: cancel.clone(),
        };
        let mut last_image = None;
        let mut last_capture = None;
        let mut stopped = false;
        loop {
            if !stopped && (cancel.load(Ordering::Acquire) || Instant::now() >= deadline) {
                let reason = if cancel.load(Ordering::Acquire) {
                    Error::Cancelled
                } else {
                    Error::TimedOut
                };
                self.stop_execution(id, reason);
                stopped = true;
            }
            match receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(call) => {
                    let response = self.app_call(
                        id,
                        &AppCall {
                            op: &call.op,
                            args: &call.args,
                            line: call.line,
                            number: call.number,
                        },
                        deadline,
                        &mut last_image,
                        &mut last_capture,
                    );
                    // A timed-out engine may have dropped its reply; the ledger
                    // already contains the original native evidence, never retry.
                    let _ = call.reply.send(response);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let joined = thread.join();
        self.settle_app_execution(id);
        let outcome = match joined {
            Ok(outcome) => outcome,
            Err(_) => {
                self.stop_execution(id, Error::Closed);
                self.abandon_execution_step(id);
                return Work::read_error(Error::BackendFailed(
                    "Script engine panicked; context closed".into(),
                ));
            }
        };
        if let Some(capture) = &mut last_capture {
            let later_calls =
                u64::from(outcome.calls).saturating_sub(capture["step"].as_u64().unwrap_or(0));
            capture["later_calls"] = json!(later_calls);
            capture["timing"] = json!(if capture.get("recovered_from").is_some() {
                "historical"
            } else if later_calls == 0 {
                "last_call"
            } else {
                "before_later_calls"
            });
        }
        let mut completed = Completed {
            value: None,
            image: last_image,
            receipt: None,
            error: None,
            payload_bytes: 0,
            payload_expired: false,
        };
        let mut state_update = None;
        let mut value = json!({"agent_protocol":agent_surface,"console":outcome.console,"console_truncated":outcome.console_truncated,"calls":outcome.calls,"last_capture":last_capture});
        match outcome.result {
            Ok(success) => {
                value["result"] = success.value;
                state_update = Some(success.state);
            }
            Err(failure) => {
                if failure.kind == "timeout" || failure.kind == "cancelled" {
                    self.stop_execution(
                        id,
                        if failure.kind == "timeout" {
                            Error::TimedOut
                        } else {
                            Error::Cancelled
                        },
                    );
                }
                let mut state = self.state.lock().unwrap();
                completed.error = Some(WireError {
                    details: None,
                    // Completed means settled, not business success. Keep the
                    // public business reason at the ordinary error boundary;
                    // script.failure still records VM kind and source location.
                    code: if agent_surface {
                        failure
                            .code
                            .clone()
                            .unwrap_or_else(|| format!("script_{}", failure.kind))
                    } else {
                        format!("script_{}", failure.kind)
                    },
                    message: failure.message.clone(),
                });
                let run = state
                    .jobs
                    .iter_mut()
                    .find(|j| j.submitted.request_id == *id)
                    .unwrap()
                    .script
                    .as_mut()
                    .unwrap();
                run.failure = Some(
                    json!({"kind":failure.kind,"message":failure.message,"line":failure.line,"operation":failure.op,"code":failure.code,"call":failure.call}),
                );
            }
        }
        self.app_execution_result(id, completed, value, state_update, agent_surface)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_unwind_joins_vm_waiting_for_a_host_reply() {
        let cancel = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel(1);
        let (signal, flag) = (cancel.clone(), finished.clone());
        let handle = std::thread::spawn(move || {
            let outcome = engine::run(
                "await app.windows();",
                json!({}),
                engine::Limits {
                    deadline: Instant::now() + Duration::from_secs(5),
                    max_calls: 1,
                    memory_bytes: MEMORY_BYTES,
                    stack_bytes: STACK_BYTES,
                },
                signal,
                sender,
            );
            flag.store(true, Ordering::Release);
            outcome
        });
        let waiting = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _owner = EngineThread {
                handle: Some(handle),
                cancel,
            };
            panic!("controlled provider unwind");
        }));
        assert!(failure.is_err());
        assert!(
            finished.load(Ordering::Acquire),
            "job unwound before VM exited"
        );
        drop(waiting);
    }
}

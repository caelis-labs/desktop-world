//! App-bound scripts reuse the outer job reservation and the existing Core.
//! The step ledger is independent of evictable result/image payloads.
use super::{
    apply_publication, error_json, is_action, script_engine as engine, wire, Command, Completed,
    Coordinator, Error, Publication, State, Submit, WireError, Work,
};
use crate::{
    computer::{ActionOutcome, Backend, FocusReturn},
    runtime::ActionReceipt,
};
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
pub(super) const MAX_STEP_RECEIPT_BYTES: usize = 1024 * 1024;
const MEMORY_BYTES: usize = 32 * 1024 * 1024;
const STACK_BYTES: usize = 512 * 1024;

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

struct Step {
    number: u32,
    line: u32,
    operation: String,
    action: bool,
    completed: bool,
    receipt: Option<ActionReceipt>,
    error: Option<WireError>,
    inventory: Option<Value>,
    discovery: Option<Value>,
}
impl Step {
    fn json(&self) -> Value {
        let mut value = json!({"number":self.number,"line":self.line,"operation":self.operation,
            "status":if self.completed {"completed"} else {"running"},
            "receipt":self.receipt.as_ref().map(wire::receipt),"error":self.error.as_ref().map(error_json),
            "inventory":self.inventory});
        if let Some(discovery) = &self.discovery {
            value["discovery"] = discovery.clone();
        }
        value
    }
}

pub(super) struct ScriptRun {
    pub deadline: Instant,
    cancel: Arc<AtomicBool>,
    steps: Vec<Step>,
    receipt_bytes: usize,
    failure: Option<Value>,
    pub state_committed: bool,
    pub(super) shutdown_error: Option<WireError>,
    focus_return: Option<FocusReturn>,
}
impl ScriptRun {
    pub fn new(deadline: Instant) -> Self {
        Self {
            deadline,
            cancel: Arc::new(AtomicBool::new(false)),
            steps: Vec::new(),
            receipt_bytes: 0,
            failure: None,
            state_committed: false,
            shutdown_error: None,
            focus_return: None,
        }
    }
    pub fn json(&self) -> Value {
        let mut value = json!({"steps":self.steps.iter().map(Step::json).collect::<Vec<_>>(),"failure":self.failure,"state_committed":self.state_committed,"shutdown_error":self.shutdown_error.as_ref().map(error_json)});
        if let Some(focus_return) = self.focus_return {
            value["focus_return"] = json!(focus_return.as_str());
        }
        value
    }
}

pub(super) fn cancel_active(state: &State) {
    if let Some(script) = state
        .jobs
        .iter()
        .find(|j| Some(j.submitted.request_id.as_str()) == state.active.as_deref())
        .and_then(|j| j.script.as_ref())
    {
        script.cancel.store(true, Ordering::Release);
    }
}

// Dropping a JoinHandle detaches it. This owner keeps the VM inside the job
// lifetime even when a Core provider panics while the VM awaits its reply.
struct EngineThread {
    handle: Option<std::thread::JoinHandle<engine::Outcome>>,
    cancel: Arc<AtomicBool>,
}
impl EngineThread {
    fn join(mut self) -> std::thread::Result<engine::Outcome> {
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
        let engine_cancel = cancel.clone();
        let thread = match std::thread::Builder::new()
            .name("dtw-app-script".into())
            .spawn(move || engine::run(&source, initial, limits, engine_cancel, sender))
        {
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
                self.stop_script(id, reason);
                stopped = true;
            }
            match receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(call) => {
                    let response = self.script_host_call(
                        id,
                        &call,
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
        let finished = self.runtime.finish_execution();
        {
            let mut state = self.state.lock().unwrap();
            let run = state
                .jobs
                .iter_mut()
                .find(|j| j.submitted.request_id == *id)
                .unwrap()
                .script
                .as_mut()
                .unwrap();
            run.focus_return = match &finished {
                Ok(status) => *status,
                Err(_) => Some(FocusReturn::Unconfirmed),
            };
        }
        if let Err(error) = finished {
            self.record_shutdown_error(Some(error), Some(id));
            self.close();
        }
        let outcome = match joined {
            Ok(outcome) => outcome,
            Err(_) => {
                self.stop_script(id, Error::Closed);
                self.abandon_script_step(id);
                return Work::read_error(Error::BackendFailed(
                    "Script engine panicked; context closed".into(),
                ));
            }
        };
        let mut completed = Completed {
            value: None,
            image: last_image,
            receipt: None,
            error: None,
            payload_bytes: 0,
            payload_expired: false,
        };
        let mut state_update = None;
        let mut value = json!({"console":outcome.console,"console_truncated":outcome.console_truncated,"calls":outcome.calls,"last_capture":last_capture});
        match outcome.result {
            Ok(success) => {
                value["result"] = success.value;
                state_update = Some(success.state);
            }
            Err(failure) => {
                if failure.kind == "timeout" || failure.kind == "cancelled" {
                    self.stop_script(
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
                    code: format!("script_{}", failure.kind),
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
        completed.value = Some(value);
        Work {
            completed,
            publication: Publication::None,
            state_update,
        }
    }

    fn script_host_call(
        &self,
        id: &str,
        call: &engine::HostCall,
        deadline: Instant,
        last_image: &mut Option<Arc<String>>,
        last_capture: &mut Option<Value>,
    ) -> Result<Value, engine::HostError> {
        let parsed = self.parse_script_call(id, call);
        let action = parsed.as_ref().is_ok_and(|s| is_action(&s.command));
        {
            let mut state = self.state.lock().unwrap();
            let run = state
                .jobs
                .iter_mut()
                .find(|j| j.submitted.request_id == id)
                .unwrap()
                .script
                .as_mut()
                .unwrap();
            if run.receipt_bytes >= MAX_STEP_RECEIPT_BYTES {
                return Err(engine::HostError::new(
                    "Step receipt memory budget exhausted; prior receipts are retained and no new operation was dispatched",
                    "receipt_limit",
                ));
            }
            run.steps.push(Step {
                number: call.number,
                line: call.line,
                operation: call.op.clone(),
                action,
                completed: false,
                receipt: None,
                error: None,
                inventory: None,
                discovery: None,
            });
        }
        let mut work = match parsed {
            Ok(submitted) => self.execute_atomic(&submitted, deadline),
            Err(error) => Work {
                completed: Completed {
                    value: None,
                    image: None,
                    receipt: None,
                    error: Some(error),
                    payload_bytes: 0,
                    payload_expired: false,
                },
                publication: Publication::None,
                state_update: None,
            },
        };
        let response = if let Some(error) = &work.completed.error {
            Err(engine::HostError {
                message: error.message.clone(),
                code: Some(error.code.clone()),
                details: error.details.clone(),
            })
        } else if let Some(receipt) = &work.completed.receipt {
            Ok(wire::receipt(receipt))
        } else {
            Ok(work.completed.value.clone().unwrap_or(Value::Null))
        };
        if let Some(image) = work.completed.image.take() {
            *last_image = Some(image);
            *last_capture = Some(json!({"step":call.number,"observation":work.completed.value}));
        }
        let mut state = self.state.lock().unwrap();
        let run = state
            .jobs
            .iter_mut()
            .find(|j| j.submitted.request_id == id)
            .unwrap()
            .script
            .as_mut()
            .unwrap();
        let step = run.steps.last_mut().unwrap();
        if call.op == "windows" && work.completed.error.is_none() {
            if let Some(value) = &work.completed.value {
                step.inventory =
                    Some(json!({"coverage":value["coverage"],"diagnostics":value["diagnostics"]}));
            }
        }
        if call.op == "observe" && work.completed.error.is_none() {
            if let Some(value) = &work.completed.value {
                if let Some(coverage) = value.get("coverage").filter(|v| v.is_object()) {
                    // Keep the read's coverage even when JS discards its page,
                    // throws later, or its result payload is evicted. Do not
                    // retain elements or cursors, or invent terminal coverage.
                    step.discovery = Some(json!({
                        "window_ref":value["window_ref"],"observation_ref":value["observation_ref"],
                        "complete":coverage["complete"],"reason":coverage["reason"],
                        "visited":coverage["visited"],
                        "has_more":value.get("next").map(|next| !next.is_null())
                    }));
                }
            }
        }
        step.receipt = work.completed.receipt;
        step.error = work.completed.error;
        step.completed = true;
        // Preserve the complete terminal receipt, including the step that fills
        // the budget. Stop before the next native operation, never truncate an
        // action's evidence or let a caught resource error reopen admission.
        run.receipt_bytes += step.json().to_string().len();
        apply_publication(&mut state, work.publication);
        response
    }

    fn parse_script_call(&self, id: &str, call: &engine::HostCall) -> Result<Submit, WireError> {
        wire::script_submit(&self.app_ref, id, &call.op, &call.args)
    }

    pub(super) fn stop_script(&self, id: &str, reason: Error) {
        {
            let mut state = self.state.lock().unwrap();
            if state.terminal.is_none() {
                state.terminal = Some(reason);
            }
            // The VM already observes this absolute deadline. Do not turn a
            // timeout into a cancellation merely to wake its interrupt handler.
            if !matches!(state.terminal, Some(Error::TimedOut)) {
                cancel_active(&state);
            }
            state.windows.clear();
            state.observations.clear();
        }
        let shutdown = self.cleanup_terminal();
        let mut state = self.state.lock().unwrap();
        if let Some(run) = state
            .jobs
            .iter_mut()
            .find(|j| j.submitted.request_id == id)
            .and_then(|j| j.script.as_mut())
        {
            if shutdown.is_some() {
                run.shutdown_error = shutdown;
            }
        }
    }

    pub(super) fn abandon_script_step(&self, id: &str) {
        let mut state = self.state.lock().unwrap();
        if let Some(run) = state
            .jobs
            .iter_mut()
            .find(|j| j.submitted.request_id == id)
            .and_then(|j| j.script.as_mut())
        {
            let active = run.steps.iter().find(|step| !step.completed);
            run.failure = Some(
                json!({"kind":"internal","message":"Execution panicked; context closed","line":active.map(|s|s.line),"operation":active.map(|s|s.operation.as_str()),"call":active.map(|s|s.number),"code":"backend_failed"}),
            );
            for step in &mut run.steps {
                if !step.completed {
                    let error = Error::BackendFailed(
                        "Host call interrupted before a final receipt; effect may be unknown"
                            .into(),
                    );
                    step.error = Some(wire::core_error(&error));
                    if step.action {
                        step.receipt = Some(ActionReceipt {
                            outcome: ActionOutcome::unknown(error),
                            provider_outcome: None,
                            interruption: Some(Error::Closed),
                        });
                    }
                    step.completed = true;
                }
            }
        }
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

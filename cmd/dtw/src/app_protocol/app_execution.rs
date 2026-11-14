//! Shared app execution: native facade dispatch, receipts and per-call settlement.
//! App/native references live in Coordinator and Runtime, never in an execution.
use super::{
    apply_publication, error_json, is_action, wire, Completed, Coordinator, Error, Publication,
    State, Work,
};
use crate::{
    computer::{ActionOutcome, Backend, FocusReturn},
    runtime::ActionReceipt,
};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};
use wire::WireError;

pub(super) const MAX_STEP_RECEIPT_BYTES: usize = 1024 * 1024;

pub(super) struct AppCall<'a> {
    pub op: &'a str,
    pub args: &'a Value,
    pub line: u32,
    pub number: u32,
}

#[derive(Debug, Clone)]
pub(super) struct HostError {
    pub message: String,
    pub code: Option<String>,
    pub details: Option<Value>,
}
impl HostError {
    pub fn new(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: Some(code.into()),
            details: None,
        }
    }
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
    resolution: Option<Value>,
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
        if let Some(resolution) = &self.resolution {
            value["resolution"] = resolution.clone();
        }
        value
    }
}

pub(super) struct AppRun {
    pub deadline: Instant,
    pub(super) cancel: Arc<AtomicBool>,
    steps: Vec<Step>,
    receipt_bytes: usize,
    pub(super) failure: Option<Value>,
    pub state_committed: bool,
    pub(super) shutdown_error: Option<WireError>,
    focus_return: Option<FocusReturn>,
    view: Option<Value>,
    snapshot: Option<Value>,
}
impl AppRun {
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
            view: None,
            snapshot: None,
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

fn scene_action(op: &str) -> bool {
    !matches!(
        op,
        "scene.look"
            | "scene.capture"
            | "scene.after"
            | "scene.readText"
            | "scene.waitForNewWindow"
            | "scene.wait"
    )
}

impl<B: Backend + 'static> Coordinator<B> {
    /// A direct operation uses the same reservation, facade, receipts and cleanup
    /// as scripts. The app scene and native bindings outlive this per-call record.
    pub(super) fn execute_app(&self, submitted: &wire::Submit) -> Work {
        let wire::Command::App { method, args, .. } = &submitted.command else {
            unreachable!()
        };
        let id = &submitted.request_id;
        let deadline = self
            .state
            .lock()
            .unwrap()
            .jobs
            .iter()
            .find(|job| job.submitted.request_id == *id)
            .unwrap()
            .script
            .as_ref()
            .unwrap()
            .deadline;
        let op = format!("scene.{method}");
        let mut last_image = None;
        let mut last_capture = None;
        let mut result = self.app_call(
            id,
            &AppCall {
                op: &op,
                args,
                line: 0,
                number: 1,
            },
            deadline,
            &mut last_image,
            &mut last_capture,
        );
        let mut calls = 1;
        let mut failure_op = op.as_str();
        let mut failure_call = 1;
        if scene_action(&op) {
            calls += 1;
            let after = self.app_call(
                id,
                &AppCall {
                    op: "scene.after",
                    args: &json!({}),
                    line: 0,
                    number: calls,
                },
                deadline,
                &mut last_image,
                &mut last_capture,
            );
            if result.is_ok() {
                if let Err(error) = after {
                    result = Err(error);
                    failure_op = "scene.after";
                    failure_call = calls;
                }
            }
        }
        self.settle_app_execution(id);
        let mut value = json!({"agent_protocol":true,"calls":calls,"last_capture":last_capture});
        let error = match result {
            Ok(result) => {
                if method == "readText" {
                    value["result"] = result;
                }
                None
            }
            Err(error) => {
                let code = error.code.unwrap_or_else(|| "operation_failed".into());
                let mut state = self.state.lock().unwrap();
                state
                    .jobs
                    .iter_mut()
                    .find(|job| job.submitted.request_id == *id)
                    .unwrap()
                    .script
                    .as_mut()
                    .unwrap()
                    .failure = Some(json!({
                    "kind":"operation","message":error.message,"line":null,
                    "operation":failure_op,"code":code,"call":failure_call
                }));
                Some(WireError {
                    code,
                    message: error.message,
                    details: error.details,
                })
            }
        };
        let completed = Completed {
            value: None,
            image: last_image,
            receipt: None,
            error,
            payload_bytes: 0,
            payload_expired: false,
        };
        // Native execution never reads or commits JavaScript state. It also
        // never clears the session's scene, observations, raster or object handles.
        self.app_execution_result(id, completed, value, None, true)
    }

    pub(super) fn settle_app_execution(&self, id: &str) {
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
    }

    pub(super) fn app_execution_result(
        &self,
        id: &str,
        mut completed: Completed,
        mut value: Value,
        state_update: Option<Value>,
        agent_surface: bool,
    ) -> Work {
        let mut state = self.state.lock().unwrap();
        let run = state
            .jobs
            .iter()
            .find(|j| j.submitted.request_id == *id)
            .unwrap()
            .script
            .as_ref()
            .unwrap();
        if let Some(step) = run
            .steps
            .last()
            .filter(|step| step.operation == "scene.after")
        {
            value["observation"] = if let Some(error) = &step.error {
                json!({"status":"failed","error":error_json(error)})
            } else if let Some(error) = run.snapshot.as_ref().and_then(|v| v["image"].get("error"))
            {
                json!({"status":"partial","error":error})
            } else {
                json!({"status":"observed"})
            };
        }
        if let Some(view) = &run.view {
            value["view"] = view.clone();
            value["snapshot"] = run.snapshot.clone().unwrap_or(Value::Null);
            if agent_surface && super::projection::shows_view(&value, completed.error.is_some()) {
                state.scene.publish();
            }
        }
        drop(state);
        completed.value = Some(value);
        Work {
            completed,
            publication: Publication::None,
            state_update,
        }
    }

    pub(super) fn check_execution_wait(&self, id: &str, deadline: Instant) -> Result<(), Error> {
        let state = self.state.lock().unwrap();
        if state
            .jobs
            .iter()
            .find(|job| job.submitted.request_id == id)
            .and_then(|job| job.script.as_ref())
            .is_some_and(|script| script.cancel.load(Ordering::Acquire))
        {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(Error::TimedOut);
        }
        Ok(())
    }

    pub(super) fn app_call(
        &self,
        id: &str,
        call: &AppCall<'_>,
        deadline: Instant,
        last_image: &mut Option<Arc<String>>,
        last_capture: &mut Option<Value>,
    ) -> Result<Value, HostError> {
        let receipt_query = call.op == "session.result";
        let parsed = wire::script_submit(&self.app_ref, id, call.op, call.args);
        let scene_call = call.op.starts_with("scene.");
        let action = if scene_call {
            scene_action(call.op)
        } else {
            parsed.as_ref().is_ok_and(|s| is_action(&s.command))
        };
        if scene_call && action {
            *last_image = None;
            *last_capture = None;
            let mut state = self.state.lock().unwrap();
            let run = state
                .jobs
                .iter_mut()
                .find(|j| j.submitted.request_id == id)
                .unwrap()
                .script
                .as_mut()
                .unwrap();
            run.view = None;
            run.snapshot = None;
        }
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
                return Err(HostError::new(
                    "Step receipt memory budget exhausted; prior receipts are retained and no new operation was dispatched",
                    "receipt_limit",
                ));
            }
            run.steps.push(Step {
                number: call.number,
                line: call.line,
                operation: call.op.into(),
                action,
                completed: false,
                receipt: None,
                error: None,
                inventory: None,
                discovery: None,
                resolution: None,
            });
        }
        let mut work = if scene_call {
            self.scene_call(id, call.op, call.args, deadline)
        } else if receipt_query {
            let reader = self
                .session
                .lock()
                .unwrap()
                .as_ref()
                .map(|s| s.read_receipt.clone());
            match reader
                .map(|reader| reader(id, call.args))
                .unwrap_or_else(|| {
                    Err(WireError {
                        code: "session_unavailable".into(),
                        message: "No MCP session is bound".into(),
                        details: None,
                    })
                }) {
                Ok(reply) => {
                    if let Some(image) = reply.image {
                        *last_image = Some(image);
                        *last_capture = Some(
                            json!({"step":call.number,"recovered_from":reply.data["execution_id"],
                            "observation":reply.data["value"]["last_capture"],"timing":"historical"}),
                        );
                    }
                    Work {
                        completed: Completed {
                            value: Some(reply.data),
                            image: None,
                            receipt: None,
                            error: None,
                            payload_bytes: 0,
                            payload_expired: false,
                        },
                        publication: Publication::None,
                        state_update: None,
                    }
                }
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
            }
        } else {
            match parsed {
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
            }
        };
        let response = if let Some(error) = &work.completed.error {
            // The implicit, read-only follow-up is a separate receipt. Let
            // the VM settle the original result even if the UI is unreadable.
            // Interruption and a closed execution still end the whole script.
            if call.op == "scene.after"
                && !matches!(
                    error.code.as_str(),
                    "closed"
                        | "cancelled"
                        | "timed_out"
                        | "timeout"
                        | "deadline_exceeded"
                        | "native_stopping"
                        | "app_changed"
                        | "wrong_app"
                        | "wrong_provider"
                        | "invalid_observation"
                )
                && Instant::now() < deadline
                && self.state.lock().unwrap().terminal.is_none()
            {
                Ok(Value::Null)
            } else {
                Err(HostError {
                    message: error.message.clone(),
                    code: Some(error.code.clone()),
                    details: error.details.clone(),
                })
            }
        } else if let Some(receipt) = &work.completed.receipt {
            let mut receipt = wire::receipt(receipt);
            if scene_call {
                // Routing is an implementation detail of the app facade. Keep
                // exact delivery evidence in the private execution ledger.
                for field in ["outcome", "provider_outcome"] {
                    if let Some(outcome) = receipt[field].as_object_mut() {
                        outcome.remove("delivery");
                    }
                }
            }
            Ok(receipt)
        } else {
            Ok(work.completed.value.clone().unwrap_or(Value::Null))
        };
        let condition_wait = call.op == "scene.wait" && call.args.get("ms").is_none();
        // Semantic reads do not cancel pixels requested earlier in this script.
        // Input, failed reads and changed surfaces still retire the old image.
        let preserve_capture = (call.op == "scene.look" || condition_wait)
            && work.completed.error.is_none()
            && work.completed.image.is_none()
            && last_image.is_some()
            && last_capture.as_ref().is_some_and(|capture| {
                capture["observation"]["windows"].is_array()
                    && work.completed.value.as_ref().is_some_and(|snapshot| {
                        capture["observation"]["windows"] == snapshot["windows"]
                    })
            });
        if scene_call
            && !preserve_capture
            && (condition_wait
                || matches!(
                    call.op,
                    "scene.look" | "scene.capture" | "scene.after" | "scene.waitForNewWindow"
                ))
        {
            *last_image = None;
            *last_capture = None;
        }
        if let Some(image) = work.completed.image.take() {
            *last_image = Some(image);
            *last_capture = Some(json!({"step":call.number,"observation":work.completed.value}));
        }
        let mut state = self.state.lock().unwrap();
        let projected = state.scene.projection();
        let text_read_failed =
            call.op == "scene.readText" && work.completed.error.is_some() && projected.is_none();
        let run = state
            .jobs
            .iter_mut()
            .find(|j| j.submitted.request_id == id)
            .unwrap()
            .script
            .as_mut()
            .unwrap();
        if scene_call
            && (condition_wait
                || matches!(
                    call.op,
                    "scene.look" | "scene.capture" | "scene.after" | "scene.waitForNewWindow"
                ))
        {
            let wait_timeout = (call.op == "scene.waitForNewWindow" || condition_wait)
                && work
                    .completed
                    .error
                    .as_ref()
                    .is_some_and(|e| e.code == "wait_timeout");
            if work.completed.error.is_some() && !wait_timeout {
                run.snapshot = None;
                run.view = None;
            } else {
                run.snapshot = work.completed.value.clone();
                run.view = projected;
                if preserve_capture {
                    if let (Some(view), Some(capture)) = (&mut run.view, &last_capture) {
                        view["image"] = capture["observation"]["image"].clone();
                        view["image"]["captured_at_call"] = capture["step"].clone();
                    }
                }
            }
        }
        if text_read_failed {
            run.snapshot = None;
            run.view = None;
            *last_image = None;
            *last_capture = None;
        }
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

    /// Replace bounded progress on the current host call, including before an
    /// input exists. Cancellation and a later action failure retain this proof.
    pub(super) fn record_resolution(&self, id: &str, progress: Value) {
        self.try_record_resolution(id, progress);
    }

    pub(super) fn try_record_resolution(&self, id: &str, progress: Value) -> bool {
        let mut state = self.state.lock().unwrap();
        if let Some(step) = state
            .jobs
            .iter_mut()
            .find(|job| job.submitted.request_id == id)
            .and_then(|job| job.script.as_mut())
            .and_then(|run| run.steps.last_mut())
            .filter(|step| !step.completed)
        {
            step.resolution = Some(progress);
            true
        } else {
            false
        }
    }

    pub(super) fn stop_execution(&self, id: &str, reason: Error) {
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

    pub(super) fn abandon_execution_step(&self, id: &str) {
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

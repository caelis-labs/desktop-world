//! Bounded QuickJS executor; every app operation is delegated to Computer Core.
// Derived from CUA 0.38.0 script_tool/engine.rs (85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19).
// Copyright (c) 2025 Cua AI, Inc. MIT; see THIRD_PARTY_NOTICES.md.
// DTW adaptations: single-app API, strict JSON state, bounded std channels and sticky limits.

use rquickjs::{context::EvalOptions, Context, Function, Persistent, Runtime, Value as JsValue};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use std::{
    cell::RefCell,
    io::{self, Write},
    rc::Rc,
};

const JSON_LIMIT: usize = 64 * 1024;
const HOST_REPLY_LIMIT: usize = 1024 * 1024;
const POLL: Duration = Duration::from_millis(5);
const CONSOLE_MAX_LINES: usize = 200;
const CONSOLE_MAX_BYTES: usize = 32 * 1024;
const CONSOLE_LINE_MAX: usize = 2_000;
pub(super) const MAX_UNHANDLED_REJECTIONS: usize = 256;

type SavedValue = Persistent<JsValue<'static>>;
/// Persistent roots preserve identity even if JS drops every reference to an
/// unhandled promise. The guard removes the callback and roots before Runtime.
struct PromiseRejections<'rt> {
    runtime: &'rt Runtime,
    pending: Rc<RefCell<Vec<(SavedValue, SavedValue)>>>,
}
impl<'rt> PromiseRejections<'rt> {
    fn new(runtime: &'rt Runtime, terminal: Arc<Mutex<Option<Failure>>>) -> Self {
        let pending = Rc::new(RefCell::new(Vec::<(SavedValue, SavedValue)>::new()));
        let tracked = pending.clone();
        runtime.set_host_promise_rejection_tracker(Some(Box::new(
            move |ctx, promise, reason, handled| {
                // No JS property access or user callbacks here: error getters may
                // have effects. Inspect only after script execution is closed.
                let promise = Persistent::save(&ctx, promise);
                let mut pending = tracked.borrow_mut();
                if let Some(index) = pending.iter().position(|(entry, _)| *entry == promise) {
                    if handled {
                        pending.remove(index);
                    }
                } else if !handled {
                    if pending.len() == MAX_UNHANDLED_REJECTIONS {
                        latch(
                            &terminal,
                            Failure::new("exception", "too many outstanding rejected promises")
                                .code("promise_rejection_limit"),
                        );
                    } else {
                        pending.push((promise, Persistent::save(&ctx, reason)));
                    }
                }
            },
        )));
        Self { runtime, pending }
    }
    fn first(&self) -> Option<SavedValue> {
        self.pending
            .borrow()
            .first()
            .map(|(_, reason)| reason.clone())
    }
}
impl Drop for PromiseRejections<'_> {
    fn drop(&mut self) {
        self.runtime.set_host_promise_rejection_tracker(None);
        self.pending.borrow_mut().clear();
    }
}

pub(super) struct Limits {
    pub deadline: Instant,
    pub max_calls: u32,
    pub memory_bytes: usize,
    pub stack_bytes: usize,
}
pub(super) struct HostCall {
    pub op: String,
    pub args: Value,
    pub line: u32,
    pub number: u32,
    pub reply: SyncSender<Result<Value, HostError>>,
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
#[derive(Debug, Clone)]
pub(super) struct Failure {
    pub kind: String,
    pub message: String,
    pub line: Option<u32>,
    pub op: Option<String>,
    pub code: Option<String>,
    pub call: Option<u32>,
}
impl Failure {
    fn new(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: bounded_message(message.into()),
            line: None,
            op: None,
            code: None,
            call: None,
        }
    }
    fn code(mut self, code: &str) -> Self {
        self.code = Some(code.into());
        self
    }
    fn at(mut self, op: &str, line: u32, number: u32) -> Self {
        self.op = Some(op.into());
        self.line = (line != 0).then_some(line);
        self.call = Some(number);
        self
    }
}
#[derive(Debug)]
pub(super) struct Success {
    pub value: Value,
    pub state: Value,
}
pub(super) struct Outcome {
    pub result: Result<Success, Failure>,
    pub console: Vec<String>,
    pub console_truncated: bool,
    pub calls: u32,
}
#[derive(Default)]
struct Console {
    lines: Vec<String>,
    bytes: usize,
    truncated: bool,
}
impl Console {
    fn push(&mut self, level: &str, text: &str) {
        if self.lines.len() >= CONSOLE_MAX_LINES || self.bytes >= CONSOLE_MAX_BYTES {
            self.truncated = true;
            return;
        }
        let mut line = if level == "log" || level == "info" {
            text.to_owned()
        } else {
            format!("[{level}] {text}")
        };
        if line.chars().count() > CONSOLE_LINE_MAX {
            line = line.chars().take(CONSOLE_LINE_MAX).collect::<String>() + "…";
            self.truncated = true;
        }
        let remaining = CONSOLE_MAX_BYTES - self.bytes;
        if line.len() > remaining {
            let mut end = remaining;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            line.truncate(end);
            self.truncated = true;
        }
        self.bytes += line.len();
        self.lines.push(line);
    }
}

/// Executes on the caller's dedicated thread. No module loader or OS bindings.
pub(super) fn run(
    script: &str,
    initial_state: Value,
    limits: Limits,
    cancel: Arc<AtomicBool>,
    calls: SyncSender<HostCall>,
) -> Outcome {
    let console = Arc::new(Mutex::new(Console::default()));
    let used = Arc::new(AtomicU32::new(0));
    let terminal = Arc::new(Mutex::new(None));
    let result = run_inner(
        script,
        initial_state,
        &limits,
        &cancel,
        calls,
        &console,
        &used,
        &terminal,
    );
    let console = std::mem::take(&mut *console.lock().unwrap_or_else(|e| e.into_inner()));
    Outcome {
        result,
        console: console.lines,
        console_truncated: console.truncated,
        calls: used.load(Ordering::SeqCst),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    script: &str,
    initial_state: Value,
    limits: &Limits,
    cancel: &Arc<AtomicBool>,
    calls: SyncSender<HostCall>,
    console: &Arc<Mutex<Console>>,
    used: &Arc<AtomicU32>,
    terminal: &Arc<Mutex<Option<Failure>>>,
) -> Result<Success, Failure> {
    if let Some(failure) = stop_reason(cancel, limits.deadline, terminal) {
        return Err(failure);
    }
    if script.len() > JSON_LIMIT {
        return Err(Failure::new("exception", "script exceeds 64 KiB").code("source_limit"));
    }
    let initial = bounded_json(&initial_state, JSON_LIMIT).map_err(|_| {
        Failure::new("exception", "initial state exceeds 64 KiB").code("state_limit")
    })?;
    validate_initial(&initial_state, 0)?;
    let internal = |error: rquickjs::Error| {
        Failure::new(
            if matches!(error, rquickjs::Error::Allocation) {
                "memory"
            } else {
                "internal"
            },
            error.to_string(),
        )
    };
    let runtime = Runtime::new().map_err(internal)?;
    runtime.set_memory_limit(limits.memory_bytes);
    runtime.set_max_stack_size(limits.stack_bytes);
    let deadline = limits.deadline;
    {
        let cancel = cancel.clone();
        let terminal = terminal.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            cancel.load(Ordering::SeqCst)
                || Instant::now() >= deadline
                || terminal.lock().unwrap_or_else(|e| e.into_inner()).is_some()
        })));
    }
    let rejections = PromiseRejections::new(&runtime, terminal.clone());
    let context = Context::full(&runtime).map_err(internal)?;
    let settled: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let reject_unhandled =
        context.with(|ctx| -> Result<Persistent<Function<'static>>, Failure> {
            let globals = ctx.globals();
            let host = {
                let cancel = cancel.clone();
                let used = used.clone();
                let terminal = terminal.clone();
                let max_calls = limits.max_calls;
                Function::new(
                    ctx.clone(),
                    move |op: String, args: String, line: u32| -> String {
                        host_call(
                            &op, &args, line, deadline, max_calls, &cancel, &used, &calls,
                            &terminal,
                        )
                    },
                )
                .map_err(internal)?
            };
            globals.set("__host", host).map_err(internal)?;
            let done = {
                let settled = settled.clone();
                let terminal = terminal.clone();
                Function::new(ctx.clone(), move |encoded: String| {
                    if encoded.len() > JSON_LIMIT * 4 + 16384 {
                        latch(
                            &terminal,
                            Failure::new("exception", "script output exceeds limit")
                                .code("output_limit"),
                        );
                        return;
                    }
                    let mut slot = settled.lock().unwrap_or_else(|e| e.into_inner());
                    if slot.is_none() {
                        *slot = Some(encoded);
                    }
                })
                .map_err(internal)?
            };
            globals.set("__done", done).map_err(internal)?;
            let log = {
                let console = console.clone();
                Function::new(ctx.clone(), move |level: String, text: String| {
                    console
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(&level, &text);
                })
                .map_err(internal)?
            };
            globals.set("__log", log).map_err(internal)?;
            globals.set("__initial_state", initial).map_err(internal)?;
            globals
                .set("__methods", super::methods::facade_metadata())
                .map_err(internal)?;
            let mut options = EvalOptions::default();
            options.global = true;
            options.strict = true;
            options.filename = Some("prelude.js".into());
            ctx.eval_with_options::<JsValue, _>(include_str!("prelude.js"), options)
                .map_err(|_| exception_failure(&ctx, cancel, deadline, terminal, false))?;
            let settle: Function = globals.get("__settle").map_err(internal)?;
            globals.remove("__settle").map_err(internal)?;
            let reject_unhandled: Function = globals.get("__reject_unhandled").map_err(internal)?;
            globals.remove("__reject_unhandled").map_err(internal)?;
            // Keep the caller's first source line on script.js line one.
            let wrapped = format!("(async () => {{\"use strict\";{script}\n}})()");
            let mut options = EvalOptions::default();
            options.global = true;
            options.strict = true;
            options.filename = Some("script.js".into());
            let promise: JsValue = ctx
                .eval_with_options(wrapped, options)
                .map_err(|_| exception_failure(&ctx, cancel, deadline, terminal, true))?;
            settle
                .call::<_, ()>((promise,))
                .map_err(|_| exception_failure(&ctx, cancel, deadline, terminal, false))?;
            Ok(Persistent::save(&ctx, reject_unhandled))
        })?;
    loop {
        if let Some(failure) = stop_reason(cancel, deadline, terminal) {
            return Err(failure);
        }
        match runtime.execute_pending_job() {
            Ok(true) => continue,
            Ok(false) => break,
            Err(error) => {
                return Err(error
                    .0
                    .with(|ctx| exception_failure(&ctx, cancel, deadline, terminal, false)))
            }
        }
    }
    if let Some(failure) = stop_reason(cancel, deadline, terminal) {
        return Err(failure);
    }
    let encoded = settled.lock().unwrap_or_else(|e| e.into_inner()).take().ok_or_else(|| Failure::new("stalled", "the script awaited a promise that never settles; this runtime has no timers or external events"))?;
    let mut outcome: Value = serde_json::from_str(&encoded)
        .map_err(|error| Failure::new("internal", format!("bad script settlement: {error}")))?;
    if outcome["ok"] == true {
        if let Some(reason) = rejections.first() {
            // Give subsequent catch handlers and derived promises the entire
            // microtask drain before deciding. Existing primary failures win.
            let encoded: String = context.with(|ctx| {
                let inspect = reject_unhandled.clone().restore(&ctx).map_err(internal)?;
                let reason = reason.restore(&ctx).map_err(internal)?;
                inspect
                    .call((reason,))
                    .map_err(|_| exception_failure(&ctx, cancel, deadline, terminal, false))
            })?;
            if let Some(failure) = stop_reason(cancel, deadline, terminal) {
                return Err(failure);
            }
            outcome = serde_json::from_str(&encoded).map_err(|error| {
                Failure::new("internal", format!("bad rejection settlement: {error}"))
            })?;
        }
    }
    if outcome["ok"] == true {
        let decode = |field: &str| -> Result<Value, Failure> {
            let text = outcome[field]
                .as_str()
                .ok_or_else(|| Failure::new("internal", "missing JSON settlement field"))?;
            if text.len() > JSON_LIMIT {
                return Err(
                    Failure::new("exception", "script JSON exceeds 64 KiB").code("output_limit")
                );
            }
            serde_json::from_str(text).map_err(|_| {
                Failure::new("exception", "script produced invalid JSON").code("invalid_json")
            })
        };
        let success = Success {
            value: decode("value_json")?,
            state: decode("state_json")?,
        };
        if let Some(failure) = stop_reason(cancel, deadline, terminal) {
            return Err(failure);
        }
        return Ok(success);
    }
    let name = outcome["name"].as_str();
    let message = outcome["message"].as_str().unwrap_or("script error");
    let oom = (name.is_none() && message == "null") || message.contains("out of memory");
    let mut failure = Failure::new(
        if oom { "memory" } else { "exception" },
        match name {
            Some(name) if name != "DtwError" => format!("{name}: {message}"),
            _ => message.into(),
        },
    );
    failure.line = outcome["line"].as_u64().and_then(|n| n.try_into().ok());
    failure.op = outcome["op"].as_str().map(str::to_owned);
    failure.code = outcome["code"].as_str().map(str::to_owned);
    failure.call = outcome["call"].as_u64().and_then(|n| n.try_into().ok());
    Err(failure)
}

#[allow(clippy::too_many_arguments)]
fn host_call(
    op: &str,
    args: &str,
    line: u32,
    deadline: Instant,
    max_calls: u32,
    cancel: &AtomicBool,
    used: &AtomicU32,
    calls: &SyncSender<HostCall>,
    terminal: &Arc<Mutex<Option<Failure>>>,
) -> String {
    let number = used.load(Ordering::SeqCst).saturating_add(1);
    let failure_reply = |failure: Failure, sticky: bool| {
        if sticky {
            latch(terminal, failure.clone());
        }
        json!({"ok":false,"error":failure.message,"code":failure.code,"call":failure.call})
            .to_string()
    };
    if let Some(failure) = stop_reason(cancel, deadline, terminal) {
        return failure_reply(failure.at(op, line, number), true);
    }
    if used.load(Ordering::SeqCst) >= max_calls {
        return failure_reply(
            Failure::new(
                "exception",
                format!("call limit reached (max_calls={max_calls})"),
            )
            .code("call_limit")
            .at(op, line, number),
            true,
        );
    }
    used.fetch_add(1, Ordering::SeqCst);
    if args.len() > JSON_LIMIT {
        return failure_reply(
            Failure::new("exception", "host arguments exceed 64 KiB")
                .code("argument_limit")
                .at(op, line, number),
            false,
        );
    }
    let args: Value = match serde_json::from_str(args) {
        Ok(value) if valid_args(op, &value) => value,
        _ => {
            return failure_reply(
                Failure::new(
                    "exception",
                    "unsupported app operation or invalid exact arguments",
                )
                .code("invalid_args")
                .at(op, line, number),
                false,
            )
        }
    };
    let (reply, receiver) = mpsc::sync_channel(1);
    let mut request = HostCall {
        op: op.into(),
        args,
        line,
        number,
        reply,
    };
    loop {
        if let Some(failure) = stop_reason(cancel, deadline, terminal) {
            return failure_reply(failure.at(op, line, number), true);
        }
        match calls.try_send(request) {
            Ok(()) => break,
            Err(TrySendError::Full(back)) => {
                request = back;
                std::thread::park_timeout(
                    POLL.min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Err(TrySendError::Disconnected(_)) => {
                return failure_reply(
                    Failure::new("internal", "Core host disconnected")
                        .code("host_disconnected")
                        .at(op, line, number),
                    true,
                )
            }
        }
    }
    loop {
        if let Some(failure) = stop_reason(cancel, deadline, terminal) {
            return failure_reply(failure.at(op, line, number), true);
        }
        match receiver.recv_timeout(POLL.min(deadline.saturating_duration_since(Instant::now()))) {
            Ok(result) => {
                if let Some(failure) = stop_reason(cancel, deadline, terminal) {
                    return failure_reply(failure.at(op, line, number), true);
                }
                return match result {
                    Ok(value) => {
                        let mut envelope = serde_json::Map::new();
                        envelope.insert("ok".into(), Value::Bool(true));
                        envelope.insert("result".into(), value);
                        match bounded_json(&Value::Object(envelope), HOST_REPLY_LIMIT) {
                            Ok(encoded) => encoded,
                            Err(_) => failure_reply(
                                Failure::new("exception", "host result exceeds 1 MiB")
                                    .code("host_result_limit")
                                    .at(op, line, number),
                                false,
                            ),
                        }
                    }
                    Err(error) => {
                        let envelope = json!({"ok":false,"error":bounded_message(error.message),
                            "code":error.code.map(|s| s.chars().take(128).collect::<String>()),
                            "call":number,"details":error.details});
                        bounded_json(&envelope, HOST_REPLY_LIMIT).unwrap_or_else(|_| {
                            failure_reply(
                                Failure::new("internal", "host error exceeds reply limit")
                                    .code("host_result_limit")
                                    .at(op, line, number),
                                false,
                            )
                        })
                    }
                };
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return failure_reply(
                    Failure::new("internal", "Core reply disconnected")
                        .code("host_disconnected")
                        .at(op, line, number),
                    true,
                )
            }
        }
    }
}
fn valid_args(op: &str, args: &Value) -> bool {
    let Some(object) = args.as_object() else {
        return false;
    };
    use super::methods::Method;
    let Some(method) = Method::parse(op) else {
        return false;
    };
    let keys: &[&str] = match method {
        Method::Windows => &[],
        Method::Observe | Method::Capture => &["window_ref"],
        Method::Press => &["window_ref", "observation_ref", "element_ref"],
        Method::Click => &["window_ref", "observation_ref", "x", "y"],
    };
    object.len() == keys.len()
        && keys.iter().all(|key| match *key {
            "x" | "y" => object
                .get(*key)
                .and_then(Value::as_f64)
                .is_some_and(f64::is_finite),
            _ => object
                .get(*key)
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty()),
        })
}
fn latch(terminal: &Mutex<Option<Failure>>, failure: Failure) {
    let mut slot = terminal.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        *slot = Some(failure);
    }
}
fn stop_reason(
    cancel: &AtomicBool,
    deadline: Instant,
    terminal: &Mutex<Option<Failure>>,
) -> Option<Failure> {
    let recorded = terminal.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if recorded.is_some() {
        return recorded;
    }
    if cancel.load(Ordering::SeqCst) {
        Some(Failure::new("cancelled", "the script was cancelled").code("cancelled"))
    } else if Instant::now() >= deadline {
        Some(Failure::new("timeout", "the script's wall-time limit was reached").code("timeout"))
    } else {
        None
    }
}
fn exception_failure(
    ctx: &rquickjs::Ctx<'_>,
    cancel: &AtomicBool,
    deadline: Instant,
    terminal: &Mutex<Option<Failure>>,
    at_parse: bool,
) -> Failure {
    let caught = ctx.catch();
    if let Some(failure) = stop_reason(cancel, deadline, terminal) {
        return failure;
    }
    let (name, message, stack) = match caught.as_exception() {
        Some(exception) => (
            exception.get::<_, String>("name").ok(),
            exception.message().unwrap_or_default(),
            exception.stack().unwrap_or_default(),
        ),
        None => (
            None,
            if caught.is_null() {
                "null".into()
            } else {
                format!("{caught:?}")
            },
            String::new(),
        ),
    };
    let kind = if name.as_deref() == Some("SyntaxError") && at_parse {
        "syntax"
    } else if message.contains("out of memory") || (caught.is_null() && name.is_none()) {
        "memory"
    } else {
        "exception"
    };
    let mut failure = Failure::new(
        kind,
        match name {
            Some(name) => format!("{name}: {message}"),
            None => message,
        },
    );
    failure.line = script_line(&stack);
    failure
}
fn bounded_message(mut message: String) -> String {
    const MARKER: &str = "… [truncated]";
    if message.len() > 8192 {
        let mut end = 8192 - MARKER.len();
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push_str(MARKER);
    }
    message
}
fn script_line(stack: &str) -> Option<u32> {
    let at = stack.find("script.js:")?;
    stack[at + "script.js:".len()..]
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}
fn validate_initial(value: &Value, depth: usize) -> Result<(), Failure> {
    if depth > 64 {
        return Err(
            Failure::new("exception", "initial state nesting exceeds 64").code("invalid_state"),
        );
    }
    match value {
        Value::Number(n) => {
            let loses_integer = n.as_i64().is_some_and(|v| (v as f64) as i128 != v as i128)
                || n.as_u64().is_some_and(|v| (v as f64) as u128 != v as u128);
            if loses_integer {
                return Err(Failure::new(
                    "exception",
                    "initial state integer is not exactly representable by JavaScript",
                )
                .code("invalid_state"));
            }
        }
        Value::Array(items) => {
            for item in items {
                validate_initial(item, depth + 1)?;
            }
        }
        Value::Object(items) => {
            for item in items.values() {
                validate_initial(item, depth + 1)?;
            }
        }
        _ => (),
    }
    Ok(())
}
struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(io::Error::other("JSON limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn bounded_json(value: &Value, limit: usize) -> Result<String, serde_json::Error> {
    let mut writer = LimitedWriter {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value)?;
    Ok(String::from_utf8(writer.bytes).expect("JSON writer produces UTF-8"))
}
#[cfg(test)]
mod tests;

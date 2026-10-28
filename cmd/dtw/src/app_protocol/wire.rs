//! Versioned agent requests and explicit Core-to-wire conversion.
//!
//! These DTOs carry references, never authorization or native bindings. The
//! coordinator resolves them in its own context and delegates admission to Core.
use crate::{computer, runtime};
use serde_json::{json, Map, Value};

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Describe,
    Submit(Submit),
    Result { request_id: String },
    Cancel { request_id: String },
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Submit {
    pub request_id: String,
    pub target: Target,
    pub command: Command,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub app_ref: String,
    pub window_ref: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Script {
        source: String,
        timeout_ms: u64,
        max_calls: u32,
    },
    Windows,
    Observe,
    Capture,
    Press {
        observation_ref: String,
        element_ref: String,
    },
    Click {
        observation_ref: String,
        x: f64,
        y: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireError {
    pub details: Option<Value>,
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for WireError {}

fn invalid(message: impl Into<String>) -> WireError {
    WireError {
        details: None,
        code: "invalid_request".into(),
        message: message.into(),
    }
}

fn object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>, WireError> {
    value
        .as_object()
        .ok_or_else(|| invalid(format!("{path} must be an object")))
}

fn fields(object: &Map<String, Value>, allowed: &[&str], path: &str) -> Result<(), WireError> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid(format!("Unknown field {path}.{key}")));
    }
    Ok(())
}

fn string(object: &Map<String, Value>, key: &str, path: &str) -> Result<String, WireError> {
    let text = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{path}.{key} must be a string")))?;
    // JSON Schema measures string length in Unicode code points, not UTF-8 bytes.
    if !(1..=128).contains(&text.chars().count()) {
        return Err(invalid(format!(
            "{path}.{key} must contain 1 to 128 characters"
        )));
    }
    Ok(text.into())
}

fn coordinate(object: &Map<String, Value>, key: &str) -> Result<f64, WireError> {
    object
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| invalid(format!("command.{key} must be a finite number")))
}

fn parse_command(value: &Value) -> Result<Command, WireError> {
    let command = object(value, "command")?;
    let kind = string(command, "kind", "command")?;
    match kind.as_str() {
        "script" => {
            fields(
                command,
                &["kind", "source", "timeout_ms", "max_calls"],
                "command",
            )?;
            let source = command
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("command.source must be a string"))?;
            if source.len() > super::scripts::MAX_SOURCE_BYTES {
                return Err(invalid("command.source exceeds 64 KiB of UTF-8"));
            }
            let (timeout_ms, max_calls) = super::scripts::budgets(command)?;
            Ok(Command::Script {
                source: source.into(),
                timeout_ms,
                max_calls,
            })
        }
        "windows" | "observe" | "capture" => {
            fields(command, &["kind"], "command")?;
            Ok(match kind.as_str() {
                "windows" => Command::Windows,
                "observe" => Command::Observe,
                _ => Command::Capture,
            })
        }
        "press" => {
            fields(
                command,
                &["kind", "observation_ref", "element_ref"],
                "command",
            )?;
            Ok(Command::Press {
                observation_ref: string(command, "observation_ref", "command")?,
                element_ref: string(command, "element_ref", "command")?,
            })
        }
        "click" => {
            fields(command, &["kind", "observation_ref", "x", "y"], "command")?;
            Ok(Command::Click {
                observation_ref: string(command, "observation_ref", "command")?,
                x: coordinate(command, "x")?,
                y: coordinate(command, "y")?,
            })
        }
        _ => Err(invalid("Unknown command.kind")),
    }
}

/// Decode one internal Core command. Parsing never resolves a native target
/// or invokes an operation.
pub fn parse_request(value: Value) -> Result<Request, WireError> {
    let request = object(&value, "request")?;
    let version = request
        .get("version")
        .and_then(Value::as_number)
        .ok_or_else(|| invalid("request.version must be the number 1"))?;
    // JSON numbers 1 and 1.0 denote the same schema version.
    if version.as_f64() != Some(1.0) {
        return Err(WireError {
            details: None,
            code: "unsupported_version".into(),
            message: "Only protocol version 1 is supported".into(),
        });
    }
    let operation = string(request, "operation", "request")?;
    match operation.as_str() {
        "describe" | "close" => {
            fields(request, &["version", "operation"], "request")?;
            Ok(if operation == "describe" {
                Request::Describe
            } else {
                Request::Close
            })
        }
        "result" | "cancel" => {
            fields(request, &["version", "operation", "request_id"], "request")?;
            let request_id = string(request, "request_id", "request")?;
            Ok(if operation == "result" {
                Request::Result { request_id }
            } else {
                Request::Cancel { request_id }
            })
        }
        "submit" => {
            fields(
                request,
                &["version", "operation", "request_id", "target", "command"],
                "request",
            )?;
            let command = parse_command(request.get("command").unwrap_or(&Value::Null))?;
            let target = object(request.get("target").unwrap_or(&Value::Null), "target")?;
            let window_ref = if matches!(command, Command::Windows | Command::Script { .. }) {
                fields(target, &["app_ref"], "target")?;
                None
            } else {
                fields(target, &["app_ref", "window_ref"], "target")?;
                Some(string(target, "window_ref", "target")?)
            };
            Ok(Request::Submit(Submit {
                request_id: string(request, "request_id", "request")?,
                target: Target {
                    app_ref: string(target, "app_ref", "target")?,
                    window_ref,
                },
                command,
            }))
        }
        _ => Err(invalid("Unknown request.operation")),
    }
}

/// Stable protocol codes. Backend diagnostics stay data, not schema names.
pub fn core_error(error: &computer::Error) -> WireError {
    use computer::Error;
    let (code, message) = match error {
        Error::Closed => ("closed", "The execution context is closed"),
        Error::Cancelled => ("cancelled", "The execution context was cancelled"),
        Error::Busy => (
            "busy",
            "The execution context already has an active operation",
        ),
        Error::WrongApp => (
            "wrong_app",
            "The target does not match the bound application instance",
        ),
        Error::UnsupportedCapability => (
            "unsupported_capability",
            "This backend does not support the capability",
        ),
        Error::StaleObservation => (
            "stale_observation",
            "The observation is expired, consumed or no longer current",
        ),
        Error::WrongProvider => (
            "wrong_provider",
            "The observation belongs to a different provider generation",
        ),
        Error::TargetChanged => (
            "target_changed",
            "The target no longer matches the observed window",
        ),
        Error::InvalidObservation => (
            "invalid_observation",
            "The observation is invalid for this operation",
        ),
        Error::InvalidPoint => (
            "invalid_point",
            "The point is outside the observation's image coordinates",
        ),
        Error::ElementNotFound => (
            "element_not_found",
            "The element is absent from this observation",
        ),
        Error::ElementNotPressable => (
            "element_not_pressable",
            "The observed element cannot be pressed",
        ),
        Error::TimedOut => ("timed_out", "The operation deadline elapsed"),
        Error::InventoryUnproven(_) => (
            "window_inventory_unproven",
            "The supported window inventory could not be proven; no absence or replay is implied",
        ),
        Error::NativeFailure { code, message } => (code.as_str(), message.as_str()),
        Error::BackendFailed(message) => ("backend_failed", message.as_str()),
    };
    WireError {
        details: match error {
            Error::InventoryUnproven(report) => Some(inventory_report(report, false)),
            Error::NativeFailure { code, .. } => Some(json!({"provider_code":code})),
            _ => None,
        },
        code: code.into(),
        message: message.into(),
    }
}

fn error_value(error: &computer::Error) -> Value {
    let error = core_error(error);
    super::error_json(&error)
}

pub(crate) fn inventory_report(report: &computer::InventoryReport, proven: bool) -> Value {
    let scope = match report.scope {
        computer::InventoryScope::SameProcessStandardWindows => "same_process_standard_windows",
    };
    let counters: serde_json::Map<String, Value> = report
        .diagnostics
        .counters
        .iter()
        .map(|(name, count)| (name.clone(), json!(count)))
        .collect();
    json!({"coverage": {
        "scope":scope,"complete_within_scope":proven,
        "status":if !proven {"unproven"} else if report.supported_count == 0 {"empty_within_scope"} else {"available"},
        "candidate_count":report.candidate_count,"supported_count":report.supported_count,"excluded_count":report.excluded_count,
        "excludes":["helper_processes","non_standard_windows","non_default_layers"]
    }, "diagnostics":{"provider":report.diagnostics.provider,"counters":counters,"error_codes":report.diagnostics.error_codes}})
}

fn outcome(outcome: &computer::ActionOutcome) -> Value {
    use computer::{DispatchState, EffectEvidence};
    let dispatch = match outcome.dispatch {
        DispatchState::NotDispatched => "not_dispatched",
        DispatchState::Dispatched => "dispatched",
        DispatchState::Unknown => "unknown",
    };
    let evidence = match &outcome.evidence {
        EffectEvidence::Unverified => json!({"kind": "unverified"}),
        EffectEvidence::Confirmed(postcondition) => {
            json!({"kind": "confirmed", "postcondition": postcondition})
        }
        EffectEvidence::Mismatch { expected, observed } => {
            json!({"kind": "mismatch", "expected": expected, "observed": observed})
        }
    };
    json!({
        "dispatch": dispatch,
        "evidence": evidence,
        "error": outcome.error.as_ref().map(error_value)
    })
}

/// Encode all evidence without reclassifying success, cancellation or dispatch.
pub fn receipt(receipt: &runtime::ActionReceipt) -> Value {
    json!({
        "outcome": outcome(&receipt.outcome),
        "provider_outcome": receipt.provider_outcome.as_ref().map(outcome),
        "interruption": receipt.interruption.as_ref().map(error_value)
    })
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties,
        "required": required, "additionalProperties": false})
}

fn identifier_schema() -> Value {
    json!({"type": "string", "minLength": 1, "maxLength": 128})
}

fn request_schema(operation: &str) -> Value {
    object_schema(
        json!({"version": {"type": "integer", "const": 1},
            "operation": {"type": "string", "const": operation}}),
        &["version", "operation"],
    )
}

fn control_schema(operation: &str) -> Value {
    let mut schema = request_schema(operation);
    schema["properties"]["request_id"] = identifier_schema();
    schema["required"] = json!(["version", "operation", "request_id"]);
    schema
}

fn submit_schema(kind: &str) -> Value {
    let mut command = object_schema(json!({"kind": {"const": kind}}), &["kind"]);
    if kind == "press" || kind == "click" {
        command["properties"]["observation_ref"] = identifier_schema();
        command["required"] = json!(["kind", "observation_ref"]);
    }
    if kind == "press" {
        command["properties"]["element_ref"] = identifier_schema();
        command["required"] = json!(["kind", "observation_ref", "element_ref"]);
    } else if kind == "click" {
        command["properties"]["x"] = json!({"type": "number"});
        command["properties"]["y"] = json!({"type": "number"});
        command["required"] = json!(["kind", "observation_ref", "x", "y"]);
    }
    if kind == "script" {
        command["properties"]["source"] = json!({"type":"string","maxLength":super::scripts::MAX_SOURCE_BYTES,"description":"JavaScript function body; also limited to 64 KiB UTF-8 and the complete transport input bound"});
        command["properties"]["timeout_ms"] =
            json!({"type":"integer","minimum":1,"maximum":super::scripts::MAX_TIMEOUT_MS});
        command["properties"]["max_calls"] =
            json!({"type":"integer","minimum":1,"maximum":super::scripts::MAX_CALLS});
        command["required"] = json!(["kind", "source"]);
    }
    let mut target = object_schema(json!({"app_ref": identifier_schema()}), &["app_ref"]);
    if kind != "windows" && kind != "script" {
        target["properties"]["window_ref"] = identifier_schema();
        target["required"] = json!(["app_ref", "window_ref"]);
    }
    let mut schema = request_schema("submit");
    schema["properties"]["request_id"] = identifier_schema();
    schema["properties"]["target"] = target;
    schema["properties"]["command"] = command;
    schema["required"] = json!(["version", "operation", "request_id", "target", "command"]);
    schema
}

/// MCP input schema for the same strict request union accepted by parse_request.
/// JSON numbers are finite; point bounds remain the Core's responsibility.
pub fn input_schema() -> Value {
    let mut variants = vec![
        request_schema("describe"),
        control_schema("result"),
        control_schema("cancel"),
        request_schema("close"),
    ];
    for kind in ["windows", "observe", "capture", "press", "click", "script"] {
        variants.push(submit_schema(kind));
    }
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "oneOf": variants
    })
}

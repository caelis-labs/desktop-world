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
    ObserveQuery(computer::ObserveQuery),
    ContinueObservation {
        cursor: String,
    },
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
    ClickWithOptions {
        observation_ref: String,
        x: f64,
        y: f64,
        options: computer::ClickOptions,
    },
    SetValue {
        observation_ref: String,
        element_ref: String,
        value: String,
    },
    Input(computer::WindowInput),
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

/// Shared argument parser for the VM bridge and the bound coordinator. Parsing
/// does not resolve references; each caller retains its own execution admission.
pub(super) fn script_submit(
    app_ref: &str,
    id: &str,
    op: &str,
    args: &Value,
) -> Result<Submit, WireError> {
    let failure = || invalid("Only bound app operations and their declared fields are permitted");
    let method = super::methods::Method::parse(op).ok_or_else(failure)?;
    let mut command = args.as_object().cloned().ok_or_else(failure)?;
    if command.contains_key("kind") {
        return Err(failure());
    }
    let mut target = json!({"app_ref":app_ref});
    if method.window_target()
        && !(matches!(method, super::methods::Method::Observe) && command.contains_key("cursor"))
    {
        target["window_ref"] = command.remove("window_ref").ok_or_else(failure)?;
    }
    command.insert("kind".into(), json!(op));
    match parse_request(
        json!({"version":1,"operation":"submit","request_id":id,"target":target,"command":command}),
    )? {
        Request::Submit(submit) => Ok(submit),
        _ => Err(failure()),
    }
}

fn text_value(
    command: &Map<String, Value>,
    key: &str,
    allow_empty: bool,
) -> Result<String, WireError> {
    command
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| {
            (allow_empty || !text.is_empty())
                && text.len() <= 16 * 1024
                && !text.contains('\0')
                // Keep the complete private worker request within 64 KiB,
                // including escaped controls and the exact native target.
                && serde_json::to_string(text).is_ok_and(|encoded| encoded.len() <= 48 * 1024)
        })
        .map(str::to_owned)
        .ok_or_else(|| {
            invalid(format!(
                "command.{key} must be a string of at most 16 KiB UTF-8 without NUL (48 KiB after JSON encoding)"
            ))
        })
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
        "observe" => {
            if command.contains_key("cursor") {
                fields(command, &["kind", "cursor"], "command")?;
                return Ok(Command::ContinueObservation {
                    cursor: string(command, "cursor", "command")?,
                });
            }
            fields(command, &["kind", "role", "text", "limit"], "command")?;
            if command.len() == 1 {
                return Ok(Command::Observe);
            }
            let filter = |key: &str, maximum: usize| -> Result<Option<String>, WireError> {
                command
                    .get(key)
                    .map(|value| {
                        value
                            .as_str()
                            .filter(|text| (1..=maximum).contains(&text.chars().count()))
                            .map(str::to_owned)
                            .ok_or_else(|| {
                                invalid(format!(
                                    "command.{key} must contain 1 to {maximum} characters"
                                ))
                            })
                    })
                    .transpose()
            };
            let limit = match command.get("limit") {
                None => computer::ObserveQuery::default().limit,
                Some(value) => value
                    .as_u64()
                    .filter(|n| (1..=100).contains(n))
                    .ok_or_else(|| invalid("command.limit must be an integer from 1 to 100"))?
                    as u32,
            };
            Ok(Command::ObserveQuery(computer::ObserveQuery {
                role: filter("role", 64)?,
                text: filter("text", 256)?,
                limit,
            }))
        }
        "windows" | "capture" => {
            fields(command, &["kind"], "command")?;
            Ok(match kind.as_str() {
                "windows" => Command::Windows,
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
            fields(
                command,
                &["kind", "observation_ref", "x", "y", "button", "count"],
                "command",
            )?;
            let observation_ref = string(command, "observation_ref", "command")?;
            let x = coordinate(command, "x")?;
            let y = coordinate(command, "y")?;
            let button = match command.get("button") {
                None => computer::MouseButton::Left,
                Some(Value::String(value)) if value == "left" => computer::MouseButton::Left,
                Some(Value::String(value)) if value == "right" => computer::MouseButton::Right,
                _ => return Err(invalid("command.button must be left or right")),
            };
            let count = match command.get("count") {
                None => 1,
                Some(value) => value
                    .as_u64()
                    .filter(|n| (1..=2).contains(n))
                    .ok_or_else(|| invalid("command.count must be 1 or 2"))?
                    as u8,
            };
            let options = computer::ClickOptions { button, count };
            if options == computer::ClickOptions::default() {
                Ok(Command::Click {
                    observation_ref,
                    x,
                    y,
                })
            } else {
                Ok(Command::ClickWithOptions {
                    observation_ref,
                    x,
                    y,
                    options,
                })
            }
        }
        "setValue" => {
            fields(
                command,
                &["kind", "observation_ref", "element_ref", "value"],
                "command",
            )?;
            Ok(Command::SetValue {
                observation_ref: string(command, "observation_ref", "command")?,
                element_ref: string(command, "element_ref", "command")?,
                value: text_value(command, "value", true)?,
            })
        }
        "focus" => {
            fields(command, &["kind"], "command")?;
            Ok(Command::Input(computer::WindowInput::Focus))
        }
        "typeText" => {
            fields(command, &["kind", "text"], "command")?;
            Ok(Command::Input(computer::WindowInput::TypeText {
                text: text_value(command, "text", false)?,
            }))
        }
        "keypress" => {
            fields(command, &["kind", "keys"], "command")?;
            let values = command
                .get("keys")
                .and_then(Value::as_array)
                .filter(|keys| (1..=8).contains(&keys.len()))
                .ok_or_else(|| invalid("command.keys must contain 1 to 8 key names"))?;
            let keys = values
                .iter()
                .map(|key| {
                    key.as_str()
                        .filter(|key| {
                            !key.is_empty()
                                && key.len() <= 32
                                && key.is_ascii()
                                && !key.chars().any(char::is_control)
                        })
                        .map(str::to_owned)
                        .ok_or_else(|| invalid("command.keys must contain ASCII key names"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Command::Input(computer::WindowInput::Keypress { keys }))
        }
        "scroll" => {
            fields(command, &["kind", "delta_x", "delta_y"], "command")?;
            let delta = |key: &str| -> Result<i32, WireError> {
                match command.get(key) {
                    None => Ok(0),
                    Some(value) => value
                        .as_i64()
                        .filter(|n| (-100..=100).contains(n))
                        .map(|n| n as i32)
                        .ok_or_else(|| {
                            invalid(format!("command.{key} must be an integer from -100 to 100"))
                        }),
                }
            };
            let (delta_x, delta_y) = (delta("delta_x")?, delta("delta_y")?);
            if delta_x == 0 && delta_y == 0 {
                return Err(invalid("scroll requires a nonzero delta"));
            }
            Ok(Command::Input(computer::WindowInput::Scroll {
                delta_x,
                delta_y,
            }))
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
            let window_ref = if matches!(
                command,
                Command::Windows | Command::Script { .. } | Command::ContinueObservation { .. }
            ) {
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
        Error::AppInventoryUnproven(_) => (
            "window_inventory_unproven",
            "No application instance provided a proven window inventory; no absence or replay is implied",
        ),
        Error::NativeFailure { code, message } => (code.as_str(), message.as_str()),
        Error::BackendFailed(message) => ("backend_failed", message.as_str()),
    };
    WireError {
        details: match error {
            Error::InventoryUnproven(report) => Some(inventory_report(report, false)),
            Error::AppInventoryUnproven(errors) => Some(super::unavailable_app_inventory(errors)),
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
    let (scope, excludes) = match report.scope {
        computer::InventoryScope::SameProcessStandardWindows => (
            "same_process_standard_windows",
            json!([
                "helper_processes",
                "non_standard_windows",
                "non_default_layers"
            ]),
        ),
        computer::InventoryScope::SameProcessLayerZeroSurfaces => (
            "same_process_layer_zero_surfaces",
            json!([
                "helper_processes",
                "non_default_layers",
                "zero_area_surfaces"
            ]),
        ),
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
        "excludes":excludes
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

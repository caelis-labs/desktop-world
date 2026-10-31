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
pub enum ScrollLocation {
    Pixels(computer::PixelPoint),
    Element(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum DiscoveryScope {
    Menu,
    Subtree {
        observation_ref: String,
        element_ref: String,
    },
    Region {
        observation_ref: String,
        region: computer::PixelRegion,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectorScope {
    Window,
    Menu,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementSelector {
    pub scope: SelectorScope,
    pub role: Option<String>,
    pub text: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Script {
        source: String,
        timeout_ms: u64,
        max_calls: u32,
    },
    Windows,
    WaitForWindow {
        state: computer::WindowWaitState,
        timeout_ms: u64,
    },
    Observe,
    ObserveQuery(computer::ObserveQuery),
    ObserveScoped {
        query: computer::ObserveQuery,
        scope: DiscoveryScope,
    },
    ReadText {
        observation_ref: String,
        element_ref: String,
        offset: u32,
        limit: u32,
    },
    ContinueObservation {
        cursor: String,
    },
    Capture,
    Find {
        selector: ElementSelector,
        timeout_ms: u64,
    },
    PressSelected {
        selector: ElementSelector,
        timeout_ms: u64,
    },
    SetValueSelected {
        selector: ElementSelector,
        timeout_ms: u64,
        value: String,
    },
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
    ClickNormalized {
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
    Scroll {
        observation_ref: String,
        anchor: ScrollLocation,
        delta_x: i32,
        delta_y: i32,
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

fn selection(command: &Map<String, Value>) -> Result<(ElementSelector, u64), WireError> {
    let selector = object(command.get("selector").unwrap_or(&Value::Null), "selector")?;
    fields(selector, &["role", "text", "scope"], "selector")?;
    let part = |key: &str, maximum: usize| {
        selector
            .get(key)
            .map(|value| {
                value
                    .as_str()
                    .filter(|text| !text.trim().is_empty() && text.chars().count() <= maximum)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        invalid(format!(
                            "selector.{key} must contain 1 to {maximum} characters"
                        ))
                    })
            })
            .transpose()
    };
    let scope = match selector.get("scope") {
        None => SelectorScope::Window,
        Some(Value::String(scope)) if scope == "window" => SelectorScope::Window,
        Some(Value::String(scope)) if scope == "menu" => SelectorScope::Menu,
        _ => return Err(invalid("selector.scope must be window or menu")),
    };
    let selector = ElementSelector {
        scope,
        role: part("role", 64)?,
        text: part("text", 256)?,
    };
    if selector.role.is_none() && selector.text.is_none() {
        return Err(invalid("selector requires role or text"));
    }
    Ok((selector, read_wait_timeout(command)?))
}

fn read_wait_timeout(command: &Map<String, Value>) -> Result<u64, WireError> {
    command.get("timeout_ms").map_or(Ok(2_000), |value| {
        value
            .as_u64()
            .filter(|timeout| *timeout <= 30_000)
            .ok_or_else(|| invalid("timeout_ms must be an integer between 0 and 30000"))
    })
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
            fields(
                command,
                &[
                    "kind",
                    "role",
                    "text",
                    "limit",
                    "scope",
                    "observation_ref",
                    "element_ref",
                    "region",
                ],
                "command",
            )?;
            if command.len() == 1 {
                return Ok(Command::Observe);
            }
            let filter =
                |key: &str, minimum: usize, maximum: usize| -> Result<Option<String>, WireError> {
                    command
                        .get(key)
                        .map(|value| {
                            value
                                .as_str()
                                .filter(|text| (minimum..=maximum).contains(&text.chars().count()))
                                .map(str::to_owned)
                                .ok_or_else(|| {
                                    invalid(format!(
                                    "command.{key} must contain {minimum} to {maximum} characters"
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
            let query = computer::ObserveQuery {
                role: filter("role", 1, 64)?,
                text: filter("text", 0, 256)?.filter(|text| !text.is_empty()),
                limit,
            };
            let scoped = command.contains_key("scope");
            let bound = command.contains_key("observation_ref");
            let element = command.contains_key("element_ref");
            let region = command.contains_key("region");
            let scope = match (scoped, bound, element, region) {
                (false, false, false, false) => return Ok(Command::ObserveQuery(query)),
                (true, false, false, false) if command["scope"] == "menu" => DiscoveryScope::Menu,
                (false, true, true, false) => DiscoveryScope::Subtree {
                    observation_ref: string(command, "observation_ref", "command")?,
                    element_ref: string(command, "element_ref", "command")?,
                },
                (false, true, false, true) => {
                    let rect = object(&command["region"], "region")?;
                    fields(rect, &["x", "y", "width", "height"], "region")?;
                    let number = |key| {
                        rect.get(key)
                            .and_then(Value::as_f64)
                            .filter(|v| v.is_finite())
                            .ok_or_else(|| invalid("region needs four finite pixel coordinates"))
                    };
                    let region = computer::PixelRegion {
                        x: number("x")?,
                        y: number("y")?,
                        width: number("width")?,
                        height: number("height")?,
                    };
                    if region.x < 0.0
                        || region.y < 0.0
                        || region.width <= 0.0
                        || region.height <= 0.0
                    {
                        return Err(invalid(
                            "region must have nonnegative origin and positive size",
                        ));
                    }
                    DiscoveryScope::Region {
                        observation_ref: string(command, "observation_ref", "command")?,
                        region,
                    }
                }
                _ => {
                    return Err(invalid(
                        "observe accepts one menu, element or capture-region scope",
                    ))
                }
            };
            Ok(Command::ObserveScoped { query, scope })
        }
        "readText" => {
            fields(
                command,
                &["kind", "observation_ref", "element_ref", "offset", "limit"],
                "command",
            )?;
            let number = |key: &str, default, minimum, maximum| {
                command.get(key).map_or(Ok(default), |v| {
                    v.as_u64()
                        .filter(|n| *n >= minimum && *n <= maximum)
                        .map(|n| n as u32)
                        .ok_or_else(|| invalid(format!("Invalid readText {key}")))
                })
            };
            Ok(Command::ReadText {
                observation_ref: string(command, "observation_ref", "command")?,
                element_ref: string(command, "element_ref", "command")?,
                offset: number("offset", 0, 0, u32::MAX as u64)?,
                limit: number("limit", 4096, 2, 16384)?,
            })
        }
        "windows" | "capture" => {
            fields(command, &["kind"], "command")?;
            Ok(match kind.as_str() {
                "windows" => Command::Windows,
                _ => Command::Capture,
            })
        }
        "waitForWindow" => {
            fields(command, &["kind", "state", "timeout_ms"], "command")?;
            let state = match command.get("state").and_then(Value::as_str) {
                Some("dialog") => computer::WindowWaitState::Dialog,
                Some("unblocked") => computer::WindowWaitState::Unblocked,
                Some("closed") => computer::WindowWaitState::Closed,
                _ => {
                    return Err(invalid(
                        "waitForWindow state must be dialog, unblocked or closed",
                    ))
                }
            };
            Ok(Command::WaitForWindow {
                state,
                timeout_ms: read_wait_timeout(command)?,
            })
        }
        "find" => {
            fields(command, &["kind", "selector", "timeout_ms"], "command")?;
            let (selector, timeout_ms) = selection(command)?;
            Ok(Command::Find {
                selector,
                timeout_ms,
            })
        }
        "press" if command.contains_key("selector") => {
            fields(command, &["kind", "selector", "timeout_ms"], "command")?;
            let (selector, timeout_ms) = selection(command)?;
            Ok(Command::PressSelected {
                selector,
                timeout_ms,
            })
        }
        "setValue" if command.contains_key("selector") => {
            fields(
                command,
                &["kind", "selector", "timeout_ms", "value"],
                "command",
            )?;
            let (selector, timeout_ms) = selection(command)?;
            Ok(Command::SetValueSelected {
                selector,
                timeout_ms,
                value: text_value(command, "value", true)?,
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
                &[
                    "kind",
                    "observation_ref",
                    "x",
                    "y",
                    "button",
                    "count",
                    "coordinate_space",
                ],
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
            match command.get("coordinate_space").and_then(Value::as_str) {
                Some("normalized_1000") => {
                    if !(0.0..1000.0).contains(&x) || !(0.0..1000.0).contains(&y) {
                        return Err(invalid(
                            "normalized_1000 coordinates must satisfy 0 <= x,y < 1000",
                        ));
                    }
                    return Ok(Command::ClickNormalized {
                        observation_ref,
                        x,
                        y,
                        options,
                    });
                }
                Some("image_pixels") => {}
                None if !command.contains_key("coordinate_space") => {}
                _ => {
                    return Err(invalid(
                        "coordinate_space must be image_pixels or normalized_1000",
                    ))
                }
            }
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
            fields(
                command,
                &[
                    "kind",
                    "delta_x",
                    "delta_y",
                    "observation_ref",
                    "x",
                    "y",
                    "element_ref",
                ],
                "command",
            )?;
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
            let shape = (
                command.contains_key("observation_ref"),
                command.contains_key("x"),
                command.contains_key("y"),
                command.contains_key("element_ref"),
            );
            let anchor = match shape {
                (false, false, false, false) => return Ok(Command::Input(computer::WindowInput::Scroll { delta_x, delta_y })),
                (true, true, true, false) => {
                    let point = computer::PixelPoint { x: coordinate(command, "x")?, y: coordinate(command, "y")? };
                    if point.x < 0.0 || point.y < 0.0 { return Err(invalid("scroll image coordinates must be nonnegative")); }
                    ScrollLocation::Pixels(point)
                },
                (true, false, false, true) => ScrollLocation::Element(string(command, "element_ref", "command")?),
                _ => return Err(invalid("scroll accepts a window center, observation_ref with x/y, or observation_ref with element_ref")),
            };
            Ok(Command::Scroll {
                observation_ref: string(command, "observation_ref", "command")?,
                anchor,
                delta_x,
                delta_y,
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
        computer::InventoryScope::SameProcessApplicationSurfaces => (
            "same_process_application_surfaces",
            json!([
                "helper_processes",
                "unproven_associations",
                "unavailable_surfaces",
                "zero_area_surfaces"
            ]),
        ),
        computer::InventoryScope::ApplicationAndProvenAssociatedSurfaces { .. } => (
            "application_and_proven_associated_surfaces",
            json!([
                "unproven_associations",
                "unavailable_surfaces",
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
    let mut value = json!({"coverage": {
        "scope":scope,"complete_within_scope":proven,
        "status":if !proven {"unproven"} else if report.supported_count == 0 {"empty_within_scope"} else {"available"},
        "candidate_count":report.candidate_count,"supported_count":report.supported_count,"excluded_count":report.excluded_count,
        "excludes":excludes
    }, "diagnostics":{"provider":report.diagnostics.provider,"counters":counters,"error_codes":report.diagnostics.error_codes}});
    if let Some(count) = report.scope.associated_count() {
        value["coverage"]["associated_count"] = json!(count);
        value["coverage"]["association_complete"] = json!(false);
    }
    value
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
    let mut value = json!({
        "dispatch": dispatch,
        "evidence": evidence,
        "error": outcome.error.as_ref().map(error_value)
    });
    if let Some(delivery) = &outcome.delivery {
        value["delivery"] = json!({
            "route": delivery.route.map(computer::InputRoute::as_str),
            "reason": delivery.reason,
            "foreground_borrowed": delivery.foreground_borrowed,
            "focus_return": delivery.focus_return.map(computer::FocusReturn::as_str)
        });
    }
    value
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

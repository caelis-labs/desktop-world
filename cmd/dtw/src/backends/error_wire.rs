//! Error codec used by the Windows/X11 worker boundary. Keep error classes
//! intact across IPC; transport must not change whether a read may degrade.
use crate::computer::Error;
use serde_json::{json, Value};

pub(crate) fn reply_error(v: &Value) -> Option<Error> {
    let code = v["error"].as_str()?;
    Some(match code {
        "target_changed" => Error::TargetChanged,
        "stale_observation" => Error::StaleObservation,
        "timed_out" => Error::TimedOut,
        "unsupported_capability" => Error::UnsupportedCapability,
        "busy" => Error::Busy,
        "invalid_point" => Error::InvalidPoint,
        "backend_failed" => Error::BackendFailed(v["message"].as_str().unwrap_or(code).into()),
        "wrong_app" => Error::WrongApp,
        "wrong_provider" => Error::WrongProvider,
        "invalid_observation" => Error::InvalidObservation,
        "cancelled" => Error::Cancelled,
        "closed" => Error::Closed,
        _ => Error::NativeFailure {
            code: code.into(),
            message: v["message"].as_str().unwrap_or(code).into(),
        },
    })
}
pub(crate) fn error_reply(error: Error, action: bool) -> Value {
    let code = match &error {
        Error::TargetChanged => "target_changed",
        Error::WrongApp => "wrong_app",
        Error::WrongProvider => "wrong_provider",
        Error::InvalidObservation => "invalid_observation",
        Error::Cancelled => "cancelled",
        Error::Closed => "closed",
        Error::BackendFailed(_) => "backend_failed",
        Error::PermissionRequired { .. } => "permission_required",
        Error::StaleObservation => "stale_observation",
        Error::TimedOut => "timed_out",
        Error::UnsupportedCapability => "unsupported_capability",
        Error::Busy => "busy",
        Error::InvalidPoint => "invalid_point",
        Error::NativeFailure { code, .. } => code,
        _ => "native_failure",
    };
    let message = match &error {
        Error::NativeFailure { message, .. }
        | Error::BackendFailed(message)
        | Error::PermissionRequired { message, .. } => message.clone(),
        _ => format!("{error:?}"),
    };
    let mut reply = json!({"error":code,"message":message});
    if action {
        reply["dispatch"] = json!("not_dispatched");
    }
    reply
}

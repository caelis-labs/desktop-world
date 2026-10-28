//! Stable platform application identifier. Execution instance details stay internal.
use super::wire::WireError;
use serde_json::{json, Value};
pub fn schema() -> Value {
    json!({"type":"string","minLength":1,"maxLength":4096,
        "description":"Application identifier returned by apps; on macOS the bundle ID, e.g. com.google.Chrome."})
}
pub fn parse(value: &Value) -> Result<String, WireError> {
    value
        .as_str()
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 4096
                && id.trim() == *id
                && !id.chars().any(char::is_control)
        })
        .map(str::to_owned)
        .ok_or_else(|| WireError {
            code: "invalid_app".into(),
            message: "app must be an application identifier from apps, e.g. com.google.Chrome"
                .into(),
            details: None,
        })
}

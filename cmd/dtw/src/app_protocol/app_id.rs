//! Stable platform application identifier. Execution instance details stay internal.
use super::wire::WireError;
use serde_json::{json, Value};
pub fn schema() -> Value {
    json!({"type":"string","minLength":1,"maxLength":4096,
        "description":"Application identifier, e.g. com.google.Chrome on macOS. Installed macOS targets may be stopped; an existing absolute .app path is also accepted."})
}
pub fn parse(value: &Value) -> Result<String, WireError> {
    value
        .as_str()
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 4096
                && id.trim() == *id
                && !id.chars().any(char::is_control)
                && !id
                    .split(['/', '\\'])
                    .any(|part| part == "." || part == "..")
                && (!id.contains('/')
                    || id.starts_with('/')
                    || id.starts_with("windows:")
                    || id.starts_with("x11:"))
        })
        .map(str::to_owned)
        .ok_or_else(|| WireError {
            code: "invalid_app".into(),
            message: "app must be an application identifier from apps, e.g. com.google.Chrome"
                .into(),
            details: None,
        })
}

//! Stable platform application identifier. Execution instance details stay internal.
use super::wire::WireError;
use serde_json::{json, Value};
pub fn schema() -> Value {
    json!({"type":"string","minLength":1,"maxLength":4096,
        "description":"Application identifier from apps; stopped installed targets can be resolved. Existing absolute .app paths on macOS and .exe paths on Windows are also accepted. Windows Win32 identifiers are normalized executable paths; packaged applications use win:<AUMID> identifiers proven by the Windows AppsFolder."})
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
                    || id.starts_with("x11:")
                    || (id.as_bytes().get(1) == Some(&b':')
                        && id.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                        && id.as_bytes().get(2) == Some(&b'/')))
        })
        .map(str::to_owned)
        .ok_or_else(|| WireError {
            code: "invalid_app".into(),
            message: "app must be an application identifier from apps, e.g. com.google.Chrome"
                .into(),
            details: None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_identifiers_keep_their_real_source_namespace() {
        for id in [
            r"C:\Program Files\Example\example.exe",
            "C:/Program Files/Example/example.exe",
            "win:Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
        ] {
            assert_eq!(parse(&json!(id)).unwrap(), id);
        }
        for id in [
            "C:/apps/../other.exe",
            "win:Family!App\n",
            "relative/app.exe",
        ] {
            assert!(parse(&json!(id)).is_err());
        }
    }
}

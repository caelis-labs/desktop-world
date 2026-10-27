//! Frozen host configuration. This is not an agent-facing grant constructor.
use super::wire::WireError;
#[cfg(any(target_os = "macos", test))]
use crate::computer::AppIdentity;
use crate::{
    computer::{AppInstance, Capability},
    runtime::Grant,
};
#[cfg(any(target_os = "macos", test))]
use serde_json::Map;
use serde_json::{json, Value};
#[cfg(any(target_os = "macos", test))]
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{path::Path, time::Instant};

#[cfg(test)]
#[path = "grant_tests.rs"]
mod tests;

#[cfg(target_os = "macos")]
const MAX_BYTES: u64 = 16 * 1024;
#[cfg(any(target_os = "macos", test))]
const MAX_VALIDITY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug)]
pub struct HostGrant {
    pub grant_id: String,
    pub app: AppInstance,
    pub capabilities: Vec<Capability>,
    pub expires_at_unix_ms: u64,
    pub expires_at: Instant,
}

impl HostGrant {
    pub fn core_grant(&self) -> Grant {
        Grant::new(self.app.clone(), self.capabilities.clone(), self.expires_at)
    }
}

fn error(code: &str, message: impl Into<String>) -> WireError {
    WireError {
        code: code.into(),
        message: message.into(),
    }
}

#[cfg(any(target_os = "macos", test))]
fn invalid(message: impl Into<String>) -> WireError {
    error("invalid_grant", message)
}

#[cfg(any(target_os = "macos", test))]
fn object<'a>(
    value: &'a Value,
    keys: &[&str],
    path: &str,
) -> Result<&'a Map<String, Value>, WireError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid(format!("{path} must be an object")))?;
    if let Some(key) = object.keys().find(|key| !keys.contains(&key.as_str())) {
        return Err(invalid(format!("Unknown field {path}.{key}")));
    }
    Ok(object)
}

#[cfg(any(target_os = "macos", test))]
fn text(object: &Map<String, Value>, key: &str, limit: usize) -> Result<String, WireError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{key} must be a string")))?;
    if !(1..=limit).contains(&value.chars().count()) {
        return Err(invalid(format!(
            "{key} must contain 1 to {limit} characters"
        )));
    }
    Ok(value.into())
}

#[cfg(any(target_os = "macos", test))]
fn unsigned(object: &Map<String, Value>, key: &str) -> Result<u64, WireError> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(format!("{key} must be an unsigned integer")))
}

// Time samples are explicit so expiry and reload behavior can be checked without
// sleeps. Production load samples them only after freezing the configuration.
#[cfg(any(target_os = "macos", test))]
fn parse(
    value: Value,
    wall_now: SystemTime,
    monotonic_now: Instant,
) -> Result<HostGrant, WireError> {
    let grant = object(
        &value,
        &[
            "version",
            "grant_id",
            "app",
            "capabilities",
            "expires_at_unix_ms",
        ],
        "grant",
    )?;
    if grant.get("version").and_then(Value::as_f64) != Some(1.0) {
        return Err(invalid("Only host grant version 1 is supported"));
    }
    let grant_id = text(grant, "grant_id", 128)?;
    let app = object(
        grant.get("app").unwrap_or(&Value::Null),
        &["identity", "pid", "generation"],
        "app",
    )?;
    let identity = AppIdentity(text(app, "identity", 4096)?);
    let pid = u32::try_from(unsigned(app, "pid")?).map_err(|_| invalid("pid exceeds u32::MAX"))?;
    if pid == 0 {
        return Err(invalid("pid must be positive"));
    }
    let generation = text(app, "generation", 20)?;
    if !generation.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid("generation must be an ASCII decimal string"));
    }
    let generation = generation
        .parse::<u64>()
        .map_err(|_| invalid("generation exceeds u64::MAX"))?;
    if generation == 0 {
        return Err(invalid("generation must be positive"));
    }
    let names = grant
        .get("capabilities")
        .and_then(Value::as_array)
        .filter(|names| !names.is_empty())
        .ok_or_else(|| invalid("capabilities must be a nonempty array"))?;
    let mut capabilities = Vec::new();
    for name in names {
        let capability = match name.as_str() {
            Some("observe") => Capability::Observe,
            Some("press") => Capability::Press,
            Some("capture") => Capability::Capture,
            Some("click") => Capability::Click,
            _ => {
                return Err(invalid(
                    "Unknown capability; expected observe, press, capture or click",
                ))
            }
        };
        if capabilities.contains(&capability) {
            return Err(invalid("Duplicate capability"));
        }
        capabilities.push(capability);
    }
    let expires_at_unix_ms = unsigned(grant, "expires_at_unix_ms")?;
    let absolute_expiry = UNIX_EPOCH
        .checked_add(Duration::from_millis(expires_at_unix_ms))
        .ok_or_else(|| invalid("expires_at_unix_ms is outside the supported clock range"))?;
    let remaining = absolute_expiry
        .duration_since(wall_now)
        .ok()
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| error("grant_expired", "The host grant has already expired"))?;
    if remaining > MAX_VALIDITY {
        return Err(invalid("Host grant expiry must be within 24 hours"));
    }
    let expires_at = monotonic_now
        .checked_add(remaining)
        .ok_or_else(|| invalid("Grant expiry is outside the monotonic clock range"))?;
    Ok(HostGrant {
        grant_id,
        app: AppInstance {
            identity,
            pid,
            generation,
        },
        capabilities,
        expires_at_unix_ms,
        expires_at,
    })
}

/// Read one private regular file through its verified descriptor and freeze it.
/// This does not prove that the configured app currently exists; trusted native
/// discovery must match the full identity and instance before creating a Runtime.
#[cfg(target_os = "macos")]
pub fn load(path: &Path) -> Result<HostGrant, WireError> {
    use std::{
        fs::OpenOptions,
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| error("grant_unreadable", format!("Cannot open host grant: {e}")))?;
    let metadata = file.metadata().map_err(|e| {
        error(
            "grant_unreadable",
            format!("Cannot inspect host grant: {e}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(invalid("Host grant must be a regular file"));
    }
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(invalid("Host grant must belong to the current user"));
    }
    if metadata.mode() & 0o7777 != 0o600 {
        return Err(invalid("Host grant permissions must be exactly 0600"));
    }
    if metadata.len() > MAX_BYTES {
        return Err(invalid("Host grant exceeds 16 KiB"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| error("grant_unreadable", format!("Cannot read host grant: {e}")))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("Host grant exceeds 16 KiB"));
    }
    let value = serde_json::from_slice(&bytes)
        .map_err(|e| invalid(format!("Host grant is not valid JSON: {e}")))?;
    // Sample monotonic time first. Pairing it with the later wall-clock sample is
    // conservative; sub-millisecond wall time is not rounded into extra lifetime.
    let monotonic_now = Instant::now();
    let wall_now = SystemTime::now();
    parse(value, wall_now, monotonic_now)
}

#[cfg(not(target_os = "macos"))]
pub fn load(_path: &Path) -> Result<HostGrant, WireError> {
    Err(error(
        "unsupported_platform",
        "Trusted native grant loading currently requires macOS",
    ))
}

pub fn app_json(app: &AppInstance) -> Value {
    json!({"identity": app.identity.0, "pid": app.pid, "generation": app.generation.to_string()})
}

pub fn capabilities_json(capabilities: &[Capability]) -> Value {
    json!(capabilities
        .iter()
        .map(|capability| match capability {
            Capability::Observe => "observe",
            Capability::Press => "press",
            Capability::Capture => "capture",
            Capability::Click => "click",
        })
        .collect::<Vec<_>>())
}

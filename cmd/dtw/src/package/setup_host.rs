//! Native host registration for an already verified immutable setup package.
mod cli;
mod dsh;
mod opencode;
mod pi;
mod plugins;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    New,
    Resume,
    Current,
}

pub(super) fn register(
    host: &str,
    install: &Value,
    previous: Option<&Value>,
    mode: &str,
) -> Result<Value, String> {
    let mode = match mode {
        "new" => Mode::New,
        "resume" => Mode::Resume,
        "current" => Mode::Current,
        _ => return Err("Unknown setup registration mode".into()),
    };
    let package = package_path(install)?;
    let result = match host {
        "dsh-desktop" => dsh::register(install, previous, mode),
        "caelis" => cli::register_caelis(package, install, previous, mode),
        "pi" => pi::register(
            package,
            previous.and_then(|recipe| package_path(recipe).ok()),
            mode,
        ),
        "opencode" => opencode::register(package, install, previous, mode),
        "agy-app" => plugins::register_agy_app(package, install, mode),
        "agy-cli" => plugins::register_agy_cli(package, install, mode),
        "grok-build" => plugins::register_grok(package, install, mode),
        _ => return Err("Unknown native setup host".into()),
    };
    Ok(result)
}

fn package_path(install: &Value) -> Result<&Path, String> {
    install["package_path"]
        .as_str()
        .map(Path::new)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| "Setup registration requires an absolute managed package path".into())
}

fn home() -> Option<PathBuf> {
    let path = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    let path = PathBuf::from(path);
    path.is_absolute().then_some(path)
}

fn variable_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

fn shell_path(path: &Path) -> String {
    let Some(path) = path
        .to_str()
        .filter(|path| !path.chars().any(char::is_control))
    else {
        return "<package path from setup output>".into();
    };
    if cfg!(windows) {
        format!("'{}'", path.replace('\'', "''"))
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

fn action(next: &str) -> Value {
    json!({"state":"host_action_required","next":next})
}

fn verify(next: &str) -> Value {
    json!({"state":"verify","next":next})
}

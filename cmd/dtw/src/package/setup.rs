//! Native setup transaction. The store owns pins; the host adapter owns registration.
use super::{
    default_store, digest, empty_binding, identifier, io_error, now, package_id, regular,
    setup_host, store_option, verify, Result, Store, SCHEMA,
};
use serde_json::{json, Value};
use std::{fs, io::ErrorKind, path::Path, path::PathBuf, process::Command};

const SETUP_SCHEMA: u64 = 1;
const DELEGATED_RESUME_ID: &str = "_DTW_SETUP_RESUME_PACKAGE_ID";
const USAGE: &str = "Usage:\n  dtw setup resume HOST PROFILE\n  dtw setup prepare HOST PROFILE PACKAGE_ID [SOURCE]\n  dtw setup confirm HOST PROFILE PACKAGE_ID\n  All commands accept --store <absolute-directory>. Use --help to show this usage.";
const HOSTS: &[&str] = &[
    "caelis",
    "dsh-desktop",
    "opencode",
    "agy-app",
    "agy-cli",
    "pi",
    "grok-build",
];

fn is_help(parts: &[&str]) -> bool {
    matches!(
        parts,
        [] | ["--help" | "-h"] | ["resume" | "prepare" | "confirm", "--help" | "-h"]
    )
}

pub fn wants_help(args: &[String]) -> bool {
    let mut args = args.to_vec();
    store_option(&mut args).is_ok() && is_help(&args.iter().map(String::as_str).collect::<Vec<_>>())
}

fn default_profile(host: &str) -> &str {
    match host {
        "agy-app" | "agy-cli" => "agy",
        _ => host,
    }
}

pub fn run(args: &[String]) -> Result<Value> {
    run_with(
        args,
        std::env::var(DELEGATED_RESUME_ID).ok().as_deref(),
        || std::env::current_exe().map_err(io_error),
        setup_host::register,
    )
}

pub(super) fn run_with(
    args: &[String],
    delegated_resume_id: Option<&str>,
    current_exe: impl Fn() -> Result<PathBuf>,
    register: impl FnOnce(&str, &Value, Option<&Value>, &str) -> std::result::Result<Value, String>,
) -> Result<Value> {
    let mut args = args.to_vec();
    let store = store_option(&mut args)?;
    let parts: Vec<_> = args.iter().map(String::as_str).collect();
    if is_help(&parts) {
        return Ok(metadata(json!({"usage":USAGE})));
    }
    let (command, host, profile, expected, source) = match parts.as_slice() {
        ["resume", host, profile] => ("resume", *host, *profile, None, None),
        [command @ ("prepare" | "confirm"), host, profile, id] => {
            (*command, *host, *profile, Some(*id), None)
        }
        ["prepare", host, profile, id, source]
            if !source.is_empty() && !source.starts_with('-') =>
        {
            ("prepare", *host, *profile, Some(*id), Some(*source))
        }
        _ => return Err(USAGE.into()),
    };
    if !HOSTS.contains(&host) {
        return Err(format!("Unsupported setup host: {host}").into());
    }
    identifier(profile)?;
    if (HOSTS.contains(&profile) && host != profile)
        || (profile == "agy" && default_profile(host) != "agy")
    {
        return Err(format!("Host profile {profile} does not belong to setup host {host}").into());
    }
    if let Some(id) = expected {
        package_id(id)?;
    }

    let root = match store {
        Some(path) => path,
        None => default_store()?,
    };
    let store = Store::open(&root).map_err(|e| e.context("open_store", command, &root, source))?;
    (|| {
        let profile_lock = store.lock_profile(profile)?;
        let mut row = store.read_host(profile)?.unwrap_or_else(|| empty_binding(profile));
        let pending = row["pending"].as_str();
        if let Some(owner) = row.get("setup_host") {
            let shared_agy_slot = profile == "agy"
                && pending.is_none()
                && matches!(owner.as_str(), Some("agy-app" | "agy-cli"));
            if owner != host && !shared_agy_slot {
                return Err(format!("Setup host does not match the owner of profile {profile}").into());
            }
        }
        if command == "resume" && delegated_resume_id.is_some() && pending != delegated_resume_id {
            return Err("Pending package changed before the selected manager could resume it".into());
        }
        if let (Some(actual), Some(expected)) = (pending, expected) {
            if actual != expected {
                return Err(format!("Pending package {actual} does not match expected package {expected}").into());
            }
        }
        let Some(id) = expected.or(pending) else {
            return Ok(metadata(json!({"state":"no_pending","host":host,"profile":profile})));
        };
        let id = id.to_owned();
        let current = row["current"].as_str();
        if command == "confirm" {
            let binding = match pending {
                Some(_) => {
                    store.recipe(&id)?;
                    row["setup_host"] = json!(host);
                    store.promote_pending(profile, row)?
                }
                None if current == Some(id.as_str()) => {
                    store.recipe(&id)?;
                    json!({"binding":row})
                }
                None => return Err(format!("No pending package to confirm for profile {profile}").into()),
            };
            let mut result = binding;
            result["state"] = json!("completed");
            result["host"] = json!(host);
            result["profile"] = json!(profile);
            result["package_id"] = json!(id);
            return Ok(metadata(result));
        }

        let mode = if pending.is_some() {
            "resume"
        } else if current == Some(id.as_str()) {
            "current"
        } else {
            "new"
        };
        let source_package = if let Some(source) = source {
            let source = Path::new(source);
            regular(source, true)?;
            let source = fs::canonicalize(source).map_err(io_error)?;
            let verified = verify(&source)?;
            if verified.id != id {
                return Err(format!("Source package {} does not match expected package {id}", verified.id).into());
            }
            let running_hash = digest(&current_exe()?)?;
            if verified.build["binary"]["sha256"] != running_hash {
                return Err("Run setup prepare with the verified source package executable".into());
            }
            Some((source, verified, running_hash))
        } else {
            None
        };

        let install = match fs::symlink_metadata(store.root.join("packages").join(&id)) {
            Ok(_) => store.recipe(&id)?,
            Err(error) if error.kind() == ErrorKind::NotFound && mode == "new" => {
                let Some((source, verified, _)) = &source_package else {
                    return Ok(metadata(json!({"state":"needs_download","host":host,"profile":profile,"package_id":id})));
                };
                store.import_verified(source, verified)?
            }
            Err(error) => return Err(io_error(error)),
        };

        let root = store.root.clone();
        let binary = Path::new(install["executable"].as_str().ok_or("Missing installed executable")?);
        if let Some((_, _, running_hash)) = source_package {
            if digest(binary)? != running_hash {
                return Err("Run setup prepare with the verified source package executable".into());
            }
        } else if mode == "new" || (mode == "resume" && row["setup_pending"] == id) {
            let running = current_exe()?;
            if fs::canonicalize(&running).map_err(io_error)? != binary
                && digest(binary)? != digest(&running)?
            {
                let _lease = store.lease(&id)?;
                // The selected manager takes its own locks and rechecks the transaction.
                drop(store);
                drop(profile_lock);
                return delegate(&root, host, profile, &install, command);
            }
        }

        let automatic = profile == default_profile(host);
        let previous = current
            .filter(|previous| automatic && *previous != id)
            .map(|previous| store.recipe(previous))
            .transpose()?;
        if mode == "new" {
            // A legacy bind can stage a pre-setup release. Only this exact
            // native pending selection is guaranteed to support delegation.
            row["setup_pending"] = json!(id);
        }
        if mode == "new" || (mode == "resume" && row.get("setup_host").is_none()) {
            row["setup_host"] = json!(host);
            row["pending"] = json!(id);
            row["updated_at"] = json!(now());
            store.update_host(profile, &row)?;
        }
        // A host can start DTW and acquire the store lock during registration.
        drop(store);
        let outcome = if automatic {
            register(host, &install, previous.as_ref(), mode)?
        } else {
            json!({
                "state":"host_action_required",
                "reason":"Automatic registration requires the host's default profile pin",
                "next":"Check and complete registration in this host profile, reload it if required, and verify its live DTW connection before confirming."
            })
        };
        prepared_state(&outcome)?;
        if Store::open(&root)?.read_host(profile)?.as_ref() != Some(&row) {
            return Err("Host binding changed during registration; retry setup for this profile".into());
        }
        let mut result = json!({"state":outcome["state"],"host":host,"profile":profile,"package_id":id,"install":install});
        for key in ["next", "reason"] {
            match outcome.get(key) {
                Some(value) if value.is_string() => result[key] = value.clone(),
                Some(value) if !value.is_null() => return Err(format!("Invalid setup host result: {key}").into()),
                _ => {}
            }
        }
        if let Some(backup) = outcome.get("backup") {
            if !backup.is_string() {
                return Err("Invalid setup host result: backup".into());
            }
            result["registration"] = json!({"backup":backup});
        }
        Ok(metadata(result))
    })()
    .map_err(|e| e.context("run_command", command, &root, source))
}

fn metadata(mut value: Value) -> Value {
    value["setup_schema"] = json!(SETUP_SCHEMA);
    value["manager_version"] = json!(env!("CARGO_PKG_VERSION"));
    value["store_schema"] = json!(SCHEMA);
    value
}

fn prepared_state(result: &Value) -> Result<()> {
    match result["state"].as_str() {
        Some("verify" | "restart_required" | "host_action_required") => Ok(()),
        _ => Err("Invalid setup host result state".into()),
    }
}

fn delegate(
    root: &Path,
    host: &str,
    profile: &str,
    install: &Value,
    command: &str,
) -> Result<Value> {
    let id = install["package_id"]
        .as_str()
        .ok_or("Missing installed package identity")?;
    let binary = install["executable"]
        .as_str()
        .ok_or("Missing installed executable")?;
    let mut child = Command::new(binary);
    child.args(["setup", command, host, profile]);
    if command == "resume" {
        // A different pending package can win the locks during process handoff.
        child.env(DELEGATED_RESUME_ID, id);
    } else {
        child.arg(id);
    }
    let output = child.arg("--store").arg(root).output().map_err(io_error)?;
    if !output.status.success() {
        let receipt: Value = serde_json::from_slice(&output.stderr).unwrap_or_default();
        let message = receipt["error"]["message"]
            .as_str()
            .unwrap_or("no setup error receipt");
        return Err(format!(
            "Selected managed executable could not {command} package {id}: {message}"
        )
        .into());
    }
    let receipt: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    prepared_state(&receipt)?;
    if receipt["setup_schema"] != SETUP_SCHEMA
        || receipt["host"] != host
        || receipt["profile"] != profile
        || receipt["package_id"] != id
        || receipt["install"] != *install
    {
        return Err("Selected managed executable returned a mismatched setup receipt".into());
    }
    Ok(receipt)
}

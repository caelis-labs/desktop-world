use super::{action, cli::HostCli, home, shell_path, variable_set, verify, Mode};
use crate::package::files;
use serde_json::Value;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

const ID: &str = "desktop-world";
const AGY_PLUGINS: &[&str] = &[".gemini", "config", "plugins"];
const AGY_OVERRIDES: &[&str] = &[
    "AGY_HOME",
    "AGY_CONFIG_DIR",
    "ANTIGRAVITY_HOME",
    "ANTIGRAVITY_CONFIG_DIR",
    "GEMINI_CLI_HOME",
    "GEMINI_CONFIG_DIR",
];
const AGY_LIST: &[&str] = &["plugin", "list", "--json"];
const AGY_WARNING: &str =
    "never loaded: this is Claude Code's file name. Move the servers to mcp_config.json";
const AGY_STOP: &str = "Quit Antigravity App and exit all agy CLI sessions sharing this slot. Wait for their Desktop World MCP servers to stop.";
const APP_ENABLE: &str = "In Antigravity App, open [Desktop World](customization://desktop-world) (Customizations → Installed) and enable Desktop World with its bundled Skill and MCP.";
const APP_RELOAD_MCP: &str = "Desktop World was updated in place. In Antigravity App, open [Desktop World](customization://desktop-world) once and restart its dtw MCP server (MCP → More actions (⋮) → Restart, or toggle Desktop World off and back on once), then rerun setup to verify.";

pub(super) fn register_agy_cli(
    package: &Path,
    install: &Value,
    previous: Option<&Value>,
    mode: Mode,
) -> Value {
    let Some(home) = default_home(AGY_OVERRIDES) else {
        return agy_scope_action(package, false);
    };
    if !agy_scope_clear(&home) {
        return agy_scope_action(package, false);
    }
    let Some(cli) = HostCli::find("agy") else {
        return action("Put the installed agy CLI on an absolute PATH outside the workspace, then rerun setup or run `agy plugin list --json` in the intended profile to inspect Desktop World.");
    };
    agy(
        package,
        install,
        previous,
        mode,
        &home,
        Some(&|args| cli.run(args)),
    )
}

pub(super) fn register_agy_app(
    package: &Path,
    install: &Value,
    previous: Option<&Value>,
    mode: Mode,
) -> Value {
    let Some(home) = default_home(AGY_OVERRIDES) else {
        return agy_scope_action(package, true);
    };
    if !agy_scope_clear(&home) {
        return agy_scope_action(package, true);
    }
    agy(package, install, previous, mode, &home, None)
}

type RunCli<'a> = &'a dyn Fn(&[&str]) -> Option<Vec<u8>>;

fn validate_agy_cli_package(package: &Path, manifest: &str, run: RunCli<'_>) -> bool {
    package.to_str().is_some_and(|path| {
        run(&["--version"]).is_some_and(|bytes| String::from_utf8_lossy(&bytes).trim() == "1.5.15")
            && run(&["plugin", "validate", path, "--json"])
                .is_some_and(|bytes| agy_validated(&bytes, package, manifest))
    })
}

fn agy_manual_slot(package: &Path, slot: &Path) -> String {
    format!(
        "Check that {} is the intended Desktop World slot. If occupied, quit Antigravity App and exit all agy CLI sessions sharing the slot. Wait for their Desktop World MCP servers to stop before you move the entire existing slot to a unique backup outside plugin discovery. Keep any backup until setup and live verification succeed. Stage a complete copy of {} outside plugin discovery, preserving file permissions. Recheck that the slot is empty and move the staged package into it with no overwrite. Recheck Desktop World and its Skill and MCP in App Customizations → Installed and, if the CLI is installed, `agy plugin list --json`. Rerun setup.",
        slot.display(),
        package.display(),
    )
}

fn agy_backup(mut receipt: Value, backup: &Path) -> Value {
    receipt["backup"] = Value::String(backup.to_string_lossy().into_owned());
    receipt
}

fn agy(
    package: &Path,
    install: &Value,
    previous: Option<&Value>,
    mode: Mode,
    home: &Path,
    cli: Option<RunCli<'_>>,
) -> Value {
    let app = cli.is_none();
    let config = home.join(".gemini/config");
    let plugins = config.join("plugins");
    let slot = plugins.join(ID);
    let slot_action = || action(&agy_manual_slot(package, &slot));
    let validation_action = || {
        action(&format!(
            "Run `agy plugin validate {} --json` in the intended CLI profile and review its Skill and MCP findings. Proceed only if the loader accepts both components. {}",
            shell_path(package),
            agy_manual_slot(package, &slot),
        ))
    };
    if owned_dirs(home, AGY_PLUGINS, false).is_err() {
        return agy_scope_action(package, app);
    }
    let Some(settings) = optional_json(&config.join("config.json")) else {
        return agy_scope_action(package, app);
    };
    let Some(mcp) = optional_json(&config.join("mcp_config.json")) else {
        return agy_scope_action(package, app);
    };
    if [ID, "dtw"]
        .iter()
        .any(|name| mcp["mcpServers"].get(name).is_some())
    {
        return action("In the intended Antigravity profile remove the duplicate standalone DTW MCP registration before using the Desktop World plugin, then rerun setup.");
    }
    let manifest = ".antigravity-plugin/plugin.json";
    let Some((source, service)) = package_identity(package, install, manifest) else {
        return action("Verify and reimport the complete package from the selected release before registering Desktop World.");
    };
    let inventory = || {
        cli.and_then(|run| run(AGY_LIST))
            .as_deref()
            .and_then(plugin_list)
    };
    let rows = if cli.is_some() {
        let Some(rows) = inventory() else {
            return action(&format!("Run `agy plugin list --json` in the intended profile and inspect the Desktop World entry before changing the shared plugin slot. {}", agy_manual_slot(package, &slot)));
        };
        rows
    } else {
        Vec::new()
    };
    let Ok(missing) = absent(&slot) else {
        return slot_action();
    };
    let backup: Option<PathBuf> = if missing {
        if mode == Mode::Current || !matches!(single_plugin(&rows), Some(None)) {
            return slot_action();
        }
        if cli.is_some_and(|run| !validate_agy_cli_package(package, manifest, run)) {
            return validation_action();
        }
        // agy 1.5.15's installer can overwrite an occupied slot absent from inventory.
        if owned_dirs(home, AGY_PLUGINS, true).is_err()
            || publish_copy(package, &source, &config, &slot).is_err()
        {
            return slot_action();
        }
        None
    } else if files::exact_copy(&slot, &source) {
        None
    } else {
        #[cfg(not(unix))]
        {
            let _ = previous;
            return slot_action();
        }
        #[cfg(unix)]
        {
            if mode == Mode::Current
                || !agy_live_swap(&source)
                || (cli.is_some()
                    && !matches!(single_plugin(&rows), Some(Some(row)) if field_path(row, "path", &slot)))
            {
                return slot_action();
            }
            let Some(prior) = previous
                .and_then(|recipe| {
                    package_identity(super::package_path(recipe).ok()?, recipe, manifest)
                })
                .map(|(verified, _)| verified)
                .filter(|prior| agy_live_swap(prior) && files::exact_copy(&slot, prior))
            else {
                return slot_action();
            };
            if cli.is_some_and(|run| !validate_agy_cli_package(package, manifest, run)) {
                return validation_action();
            }
            if owned_dirs(home, AGY_PLUGINS, false).is_err() {
                return agy_scope_action(package, app);
            }
            match replace_copy_with(package, &source, &prior, &config, &slot, swap_directory) {
                Ok(backup) => Some(backup),
                Err(ReplaceError::Aborted) => return slot_action(),
                Err(ReplaceError::Busy) => {
                    return action("Another setup is updating the shared Antigravity slot. Rerun setup after it finishes.");
                }
                Err(ReplaceError::Preserved(preserved)) => {
                    return agy_backup(action(&format!(
                        "The Desktop World slot {} changed concurrently during update. The displaced entry is at {}. {AGY_STOP} Inspect both paths and rerun setup. Keep the backup until live verification succeeds. Restore it only to an empty slot after rechecking the intended registration.",
                        slot.display(),
                        preserved.display(),
                    )), &preserved);
                }
            }
        }
    };
    let changed = missing || backup.is_some();
    let check_registration = || {
        let rows = if changed && cli.is_some() {
            let Some(after) = inventory() else {
                return action("Run `agy plugin list --json` to confirm the installed Desktop World copy and its Skill and MCP, then rerun setup.");
            };
            after
        } else {
            rows
        };
        if changed && !files::exact_copy(&slot, &source) {
            return slot_action();
        }
        let binary = slot.join(
            source.build["binary"]["path"]
                .as_str()
                .expect("verified binary path"),
        );
        if app {
            let live = live_check(
                "Open a new Antigravity App conversation in the intended workspace.",
                service,
                &binary,
            );
            if settings["plugins"][ID]["enabled"] == true {
                if backup.is_some() {
                    return action(APP_RELOAD_MCP);
                }
                return verify(&live);
            }
            return action(&format!("{APP_ENABLE} {live}"));
        }
        let Some(Some(row)) = single_plugin(&rows) else {
            return slot_action();
        };
        if !field_path(row, "path", &slot)
            || row["version"] != install["version"]
            || !one_named(&row["skills"], service)
            || !one_named(&row["mcpServers"], service)
        {
            return slot_action();
        }
        if row["enabled"] != true {
            return action("Run `agy plugin enable desktop-world` in the default profile, then rerun setup to check the installed copy and its components.");
        }
        verify(&live_check(
            "Start a new agy CLI session in the intended workspace.",
            service,
            &binary,
        ))
    };
    let mut receipt = check_registration();
    if let Some(path) = &backup {
        receipt["next"] = Value::String(format!(
            "{} Keep the displaced copy at {} until setup and live verification succeed.",
            receipt["next"].as_str().expect("registration next step"),
            path.display(),
        ));
        receipt = agy_backup(receipt, path);
    }
    receipt
}

#[cfg(unix)]
fn agy_live_swap(package: &files::Verified) -> bool {
    package.build["agy_live_swap"].as_u64() == Some(1)
}

fn agy_scope_action(package: &Path, app: bool) -> Value {
    if app {
        action(&format!("In the intended App workspace check Customizations → Installed for Desktop World. Choose one discovery scope, `.agents/plugins/desktop-world` in that workspace or `~/.gemini/config/plugins/desktop-world` globally, and copy the complete package from {} into the empty slot. Then enable Desktop World under Installed.", package.display()))
    } else {
        action(&format!("In the intended CLI profile run `agy plugin list --json` and check for a workspace or global Desktop World copy. Resolve duplicate copies before validating the package with `agy plugin validate {} --json`.", shell_path(package)))
    }
}

fn agy_scope_clear(home: &Path) -> bool {
    owned_dirs(home, &[".agents"], false).is_ok()
        && absent(&home.join(".agents/plugins")).is_ok_and(|missing| missing)
        && workspace_clear(home, &[&[".agents", "plugins"], &[".gemini"]])
}

pub(super) fn register_grok(package: &Path, install: &Value, mode: Mode) -> Value {
    let Some(home) = default_home(&["GROK_HOME", "GROK_CONFIG", "GROK_CONFIG_PATH"]) else {
        return grok_action(
            package,
            "Check the Desktop World copy in the intended Grok profile first.",
        );
    };
    if !workspace_clear(&home, &[&[".grok"], &[".claude"], &[".mcp.json"]])
        || owned_dirs(&home, &[".grok", "installed-plugins"], false).is_err()
        || [".grok/config.toml", ".grok/installed-plugins/registry.json"]
            .iter()
            .any(|file| !regular_or_missing(&home.join(file)))
    {
        return grok_action(
            package,
            "Check the Desktop World copy in the intended Grok project or profile first.",
        );
    }
    let Some(cli) = HostCli::find("grok") else {
        return grok_action(
            package,
            "Use the installed Grok Build CLI for the intended profile.",
        );
    };
    grok(package, install, mode, &home, &|args| cli.run(args))
}

fn grok(package: &Path, install: &Value, mode: Mode, home: &Path, run: RunCli<'_>) -> Value {
    let fallback = |reason| grok_action(package, reason);
    let Some(rows) = run(&["plugin", "list", "--json"])
        .as_deref()
        .and_then(plugin_list)
    else {
        return fallback("Run `grok plugin list --json` to identify the existing Desktop World copy before changing it.");
    };
    let row = match single_plugin(&rows) {
        Some(Some(row)) => row,
        Some(None) if mode == Mode::Current => return fallback("The current Desktop World registration is absent from the default plugin inventory. Restore it only after checking the intended profile."),
        Some(None) => return fallback("Desktop World is absent from the default plugin inventory."),
        None => return fallback("Resolve duplicate Desktop World registrations in the intended Grok profile before changing the plugin."),
    };
    let inspect_copy = || {
        fallback("Inspect `grok plugin details desktop-world` and `grok inspect --json`. Resolve the existing Desktop World copy in the intended profile before reinstalling.")
    };
    let Some(key) = row["repo_key"].as_str().filter(|key| {
        !key.is_empty()
            && key.len() <= 128
            && key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
    }) else {
        return inspect_copy();
    };
    let copy = home.join(".grok/installed-plugins").join(key);
    let canonical_source = fs::canonicalize(package).ok();
    if row["status"] != "installed"
        || row["version"] != install["version"]
        || !field_path(row, "path", &copy)
        || !(field_path(row, "source", package)
            || canonical_source
                .as_ref()
                .is_some_and(|path| field_path(row, "source", path)))
        || !same_copy(&copy, install)
    {
        return inspect_copy();
    }
    let Some((source, service)) = package_identity(package, install, ".claude-plugin/plugin.json")
    else {
        return inspect_copy();
    };
    let Some(binary) = source.build["binary"]["path"].as_str() else {
        return inspect_copy();
    };
    if !run(&["inspect", "--json"])
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .is_some_and(|inspect| grok_discovery(&inspect, &copy, service, binary))
    {
        return inspect_copy();
    }
    verify(&live_check(
        "Start a new Grok Build session in the intended workspace.",
        service,
        &copy.join(binary),
    ))
}

fn live_check(fresh: &str, service: &str, binary: &Path) -> String {
    format!("{fresh} Load the bundled {service} Skill, confirm the host launched {}, and use live apps and exec to observe and capture an authorized test window.", binary.display())
}

fn grok_action(package: &Path, reason: &str) -> Value {
    let path = shell_path(package);
    action(&format!("{reason} When the intended profile has an empty slot, run `grok plugin validate {path}`. If you approve trusting this package, run `grok plugin install {path} --trust`, then rerun setup to check the installed copy and discovered Skill and MCP."))
}

fn grok_discovery(inspect: &Value, copy: &Path, service: &str, binary: &str) -> bool {
    let source = |row: &Value, path: &Path| {
        row["source"]["type"] == "plugin"
            && row["source"]["plugin_name"] == ID
            && field_path(&row["source"], "path", path)
    };
    let skill_path = copy.join("skills").join(service).join("SKILL.md");
    matches!(inspect["plugins"].as_array().and_then(|rows| single_plugin(rows)), Some(Some(row))
        if row["enabled"] == true && row["scope"] == "user" && field_path(row, "path", copy))
        && matches!(inspect["skills"].as_array().and_then(|rows| single_named(rows, service)), Some(Some(row))
            if source(row, &skill_path))
        && matches!(inspect["mcpServers"].as_array().and_then(|rows| single_named(rows, service)), Some(Some(row))
            if row["transport"] == "stdio" && source(row, copy) && field_path(row, "target", &copy.join(binary)))
}

fn package_identity(
    package: &Path,
    install: &Value,
    manifest: &str,
) -> Option<(files::Verified, &'static str)> {
    let verified = files::verify(package).ok()?;
    let service = match verified.build["package_store_schema"].as_u64()? {
        1 => ID,
        2 => "dtw",
        _ => return None,
    };
    (install["package_id"] == verified.id
        && install["version"] == verified.build["version"]
        && files::json(&package.join(manifest)).ok()?["name"] == ID)
        .then_some((verified, service))
}

fn same_copy(path: &Path, install: &Value) -> bool {
    files::verify(path).is_ok_and(|copy| {
        install["package_id"] == copy.id && install["version"] == copy.build["version"]
    })
}

fn agy_validated(bytes: &[u8], package: &Path, manifest: &str) -> bool {
    let Ok(report) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    report["name"] == ID
        && report["manifest"] == manifest
        && field_path(&report, "dir", package)
        && report["components"]["skills"] == 1
        && report["components"]["mcpServers"] == 1
        && report["findings"].as_array().is_some_and(|findings| {
            findings.iter().all(|row| {
                row["severity"] == "note"
                    || (row["severity"] == "warning"
                        && row["file"] == ".mcp.json"
                        && row["message"] == AGY_WARNING)
            })
        })
}

fn absent(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

// Walk fixed path segments without opening a config through a symlink or junction.
fn owned_dirs(base: &Path, parts: &[&str], create: bool) -> io::Result<()> {
    let mut path = base.to_path_buf();
    for part in parts {
        path.push(part);
        if absent(&path)? {
            if !create {
                return Ok(());
            }
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        let metadata = fs::symlink_metadata(&path)?;
        if files::linked(&metadata) || !metadata.is_dir() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
    }
    Ok(())
}

fn workspace_clear(home: &Path, checks: &[&[&str]]) -> bool {
    let Ok(cwd) = env::current_dir().and_then(fs::canonicalize) else {
        return false;
    };
    workspace_clear_from(&cwd, home, checks)
}

fn workspace_clear_from(cwd: &Path, home: &Path, checks: &[&[&str]]) -> bool {
    cwd.ancestors()
        .take_while(|path| *path != home)
        .all(|root| {
            checks.iter().all(|parts| {
                let (leaf, parent) = parts.split_last().expect("fixed nonempty scope path");
                owned_dirs(root, parent, false).is_ok()
                    && absent(
                        &parent
                            .iter()
                            .fold(root.to_path_buf(), |path, part| path.join(part))
                            .join(leaf),
                    )
                    .is_ok_and(|missing| missing)
            })
        })
}

fn default_home(overrides: &[&str]) -> Option<PathBuf> {
    if overrides.iter().any(|name| variable_set(name)) {
        return None;
    }
    fs::canonicalize(home()?).ok().filter(|path| path.is_dir())
}

fn regular_or_missing(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) => metadata.is_file() && !files::linked(&metadata),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

fn optional_json(path: &Path) -> Option<Value> {
    if absent(path).ok()? {
        return Some(Value::Null);
    }
    files::json(path).ok().filter(Value::is_object)
}

fn plugin_list(bytes: &[u8]) -> Option<Vec<Value>> {
    let rows: Vec<Value> = serde_json::from_slice(bytes).ok()?;
    rows.iter()
        .all(|row| row["name"].as_str().is_some_and(|name| !name.is_empty()))
        .then_some(rows)
}

fn single_plugin(rows: &[Value]) -> Option<Option<&Value>> {
    single_named(rows, ID)
}

fn single_named<'a>(rows: &'a [Value], name: &str) -> Option<Option<&'a Value>> {
    let mut matching = rows.iter().filter(|row| row["name"] == name);
    let first = matching.next();
    matching.next().is_none().then_some(first)
}

fn one_named(list: &Value, name: &str) -> bool {
    list.as_array()
        .is_some_and(|items| items.len() == 1 && items[0] == name)
}

fn field_path(row: &Value, field: &str, expected: &Path) -> bool {
    row[field].as_str().is_some_and(|value| {
        if cfg!(windows) {
            let expected = expected.to_string_lossy();
            return value
                .strip_prefix(r"\\?\")
                .unwrap_or(value)
                .replace('/', "\\")
                == expected
                    .strip_prefix(r"\\?\")
                    .unwrap_or(&expected)
                    .replace('/', "\\");
        }
        Path::new(value) == expected
    })
}

fn stage_copy(package: &Path, source: &files::Verified, config: &Path) -> io::Result<PathBuf> {
    // Stage under config so the host only discovers published copies in plugins/.
    let stage = config.join(format!(".dtw-setup-{}", uuid::Uuid::new_v4()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&stage)?;
    }
    #[cfg(windows)]
    fs::create_dir(&stage)?;
    if files::copy(package, &stage.join(ID), source).is_err() {
        let _ = fs::remove_dir_all(&stage);
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(stage)
}

fn publish_copy(
    package: &Path,
    source: &files::Verified,
    config: &Path,
    slot: &Path,
) -> io::Result<()> {
    let stage = stage_copy(package, source, config)?;
    let result = rename_without_replace(&stage.join(ID), slot);
    let _ = fs::remove_dir_all(&stage);
    result
}

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
enum ReplaceError {
    Aborted,
    Busy,
    Preserved(PathBuf),
}

#[cfg(unix)]
fn replace_copy_with(
    package: &Path,
    source: &files::Verified,
    previous: &files::Verified,
    config: &Path,
    slot: &Path,
    swap: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<PathBuf, ReplaceError> {
    // Setup from any store or either host surface shares this slot lock.
    let lock = files::open_lock_file(&config.join(".dtw-setup.lock"))
        .map_err(|_| ReplaceError::Aborted)?;
    lock.try_lock().map_err(|error| match error {
        fs::TryLockError::WouldBlock => ReplaceError::Busy,
        fs::TryLockError::Error(_) => ReplaceError::Aborted,
    })?;
    let stage = stage_copy(package, source, config).map_err(|_| ReplaceError::Aborted)?;
    let displaced = stage.join(ID);
    if !files::exact_copy(slot, previous) || swap(&displaced, slot).is_err() {
        let _ = fs::remove_dir_all(&stage);
        return Err(ReplaceError::Aborted);
    }
    // App, CLI, and manual writers do not take the setup lock. A second exchange
    // could displace a new occupant, and open handles could still modify the backup.
    if !files::exact_copy(&displaced, previous) || !files::exact_copy(slot, source) {
        return Err(ReplaceError::Preserved(displaced));
    }
    Ok(displaced)
}

#[cfg(unix)]
fn unix_rename(from: &Path, to: &Path, flags: libc::c_uint) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // Both paths are NUL-terminated and live for the duration of this call.
    #[cfg(target_os = "macos")]
    let status = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), flags) };
    #[cfg(target_os = "linux")]
    let status = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            flags,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn rename_without_replace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let flags = libc::RENAME_EXCL;
    #[cfg(target_os = "linux")]
    let flags = libc::RENAME_NOREPLACE;
    unix_rename(from, to, flags)
}

#[cfg(unix)]
fn swap_directory(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let flags = libc::RENAME_SWAP;
    #[cfg(target_os = "linux")]
    let flags = libc::RENAME_EXCHANGE;
    unix_rename(from, to, flags)
}

#[cfg(windows)]
fn rename_without_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{MoveFileExW, MOVE_FILE_FLAGS},
    };
    let from: Vec<u16> = from.as_os_str().encode_wide().chain([0]).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain([0]).collect();
    // With no flags, an existing destination is an error and cross-volume moves are disallowed.
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    }
    .map_err(|_| io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
        package: PathBuf,
        install: Value,
    }

    impl Fixture {
        fn new(schema: u64) -> Self {
            Self::with_capability(schema, None)
        }

        fn with_capability(schema: u64, capability: Option<Value>) -> Self {
            let root = env::temp_dir().join(format!("dtw-plugin-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let root = fs::canonicalize(root).unwrap();
            let home = root.join("home");
            fs::create_dir(&home).unwrap();
            let package = root.join("managed/package");
            let install = Self::build_package(&package, schema, "0.6.0", capability);
            Self {
                root,
                home,
                package,
                install,
            }
        }

        fn build_package(
            package: &Path,
            schema: u64,
            version: &str,
            capability: Option<Value>,
        ) -> Value {
            let service = if schema == 1 { ID } else { "dtw" };
            let skill = package.join("skills").join(service);
            fs::create_dir_all(&skill).unwrap();
            fs::create_dir(package.join("bin")).unwrap();
            let binary = package.join("bin/dtw");
            fs::write(&binary, format!("fixture binary {version}")).unwrap();
            for name in ["SKILL.md", "API.md", "PATTERNS.md"] {
                fs::write(skill.join(name), format!("{version} {name}")).unwrap();
            }
            for name in [
                "plugin.json",
                "package.json",
                ".antigravity-plugin/plugin.json",
                ".claude-plugin/plugin.json",
            ] {
                let manifest = package.join(name);
                fs::create_dir_all(manifest.parent().unwrap()).unwrap();
                files::write(&manifest, &json!({"name":ID,"version":version})).unwrap();
            }
            let mut build = json!({
                "package_store_schema":schema,"version":version,"target":"aarch64-apple-darwin",
                "binary":{"path":"bin/dtw","sha256":files::digest(&binary).unwrap()},
                "skill_sha256":files::digest(&skill.join("SKILL.md")).unwrap()
            });
            if let Some(capability) = capability {
                build["agy_live_swap"] = capability;
            }
            files::write(&package.join("BUILD.json"), &build).unwrap();
            let sums: String = files::inventory(package)
                .unwrap()
                .iter()
                .map(|(name, path)| format!("{}  {name}\n", files::digest(path).unwrap()))
                .collect();
            fs::write(package.join("SHA256SUMS"), sums).unwrap();
            let id = files::verify(package).unwrap().id;
            json!({"package_id":id,"version":version,"package_path":package})
        }

        fn add_package(&self, name: &str, schema: u64, version: &str) -> (PathBuf, Value) {
            let package = self.root.join("managed").join(name);
            let install = Self::build_package(&package, schema, version, Some(json!(1)));
            (package, install)
        }

        fn set_app_enabled(&self, enabled: bool) {
            let config = self.home.join(".gemini/config/config.json");
            fs::create_dir_all(config.parent().unwrap()).unwrap();
            fs::write(
                &config,
                serde_json::to_vec(&json!({"plugins": {ID: {"enabled": enabled}}})).unwrap(),
            )
            .unwrap();
        }

        fn slot(&self) -> PathBuf {
            self.home.join(".gemini/config/plugins/desktop-world")
        }

        fn copy(&self, target: &Path) {
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            files::copy(
                &self.package,
                target,
                &files::verify(&self.package).unwrap(),
            )
            .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn assert_stopped_replacement(receipt: &Value) {
        assert_eq!(receipt["state"], "host_action_required");
        let next = receipt["next"].as_str().unwrap();
        for step in [
            "If occupied, quit Antigravity App and exit all agy CLI sessions",
            "move the entire existing slot to a unique backup outside plugin discovery",
            "Keep any backup until setup and live verification succeed",
            "Recheck that the slot is empty",
            "with no overwrite",
            "agy plugin list --json",
            "Rerun setup",
        ] {
            assert!(next.contains(step), "missing {step:?} in {next:?}");
        }
    }

    fn output(value: Value) -> Option<Vec<u8>> {
        Some(serde_json::to_vec(&value).unwrap())
    }

    #[test]
    fn agy_cli_publishes_a_complete_copy_then_reuses_the_matching_slot() {
        let f = Fixture::new(2);
        let slot = f.slot();
        let calls = RefCell::new(Vec::new());
        let run = |args: &[&str]| {
            calls
                .borrow_mut()
                .push(args[0..args.len().min(2)].join(" "));
            match args {
                AGY_LIST if !slot.exists() => output(json!([])),
                AGY_LIST => output(
                    json!([{"name":ID,"path":slot,"enabled":true,"version":"0.6.0","skills":["dtw"],"mcpServers":["dtw"]}]),
                ),
                ["--version"] => Some(b"1.5.15\n".to_vec()),
                ["plugin", "validate", _, "--json"] => output(json!({
                    "dir":f.package,"name":ID,"manifest":".antigravity-plugin/plugin.json",
                    "components":{"skills":1,"mcpServers":1},
                    "findings":[{"severity":"warning","file":".mcp.json","message":AGY_WARNING}]
                })),
                _ => panic!("unexpected {args:?}"),
            }
        };
        let result = agy(&f.package, &f.install, None, Mode::New, &f.home, Some(&run));
        assert_eq!(result["state"], "verify");
        assert!(same_copy(&slot, &f.install));
        assert_eq!(
            *calls.borrow(),
            ["plugin list", "--version", "plugin validate", "plugin list"]
        );
        calls.borrow_mut().clear();
        let result = agy(
            &f.package,
            &f.install,
            None,
            Mode::Current,
            &f.home,
            Some(&run),
        );
        assert_eq!(result["state"], "verify");
        assert_eq!(*calls.borrow(), ["plugin list"]);
    }

    #[test]
    fn agy_never_replaces_invisible_or_partial_slots() {
        let f = Fixture::new(2);
        let slot = f.slot();
        fs::create_dir_all(slot.join(".antigravity-plugin")).unwrap();
        fs::write(slot.join("foreign"), "preserve").unwrap();
        let run = |args: &[&str]| {
            assert_eq!(args, AGY_LIST);
            output(json!([]))
        };
        for mode in [Mode::New, Mode::Resume, Mode::Current] {
            let result = agy(&f.package, &f.install, None, mode, &f.home, Some(&run));
            assert_eq!(result["state"], "host_action_required");
            assert_eq!(
                fs::read_to_string(slot.join("foreign")).unwrap(),
                "preserve"
            );
        }
        let source = files::verify(&f.package).unwrap();
        let config = f.home.join(".gemini/config");
        assert!(publish_copy(&f.package, &source, &config, &slot).is_err());
        assert_eq!(fs::read_dir(config).unwrap().count(), 1);
        assert_eq!(fs::read_dir(slot.parent().unwrap()).unwrap().count(), 1);
        assert_eq!(
            fs::read_to_string(slot.join("foreign")).unwrap(),
            "preserve"
        );
    }

    #[test]
    fn agy_workspace_plugin_discovery_prevents_a_global_write() {
        let f = Fixture::new(2);
        let workspace = f.home.join("workspace");
        let nested = workspace.join("nested");
        fs::create_dir_all(&nested).unwrap();
        let checks: &[&[&str]] = &[&[".agents", "plugins"], &[".gemini"]];
        assert!(workspace_clear_from(&nested, &f.home, checks));
        f.copy(&workspace.join(".agents/plugins/desktop-world"));
        assert!(!workspace_clear_from(&nested, &f.home, checks));
        assert!(!f.slot().exists());
    }

    #[test]
    fn agy_never_follows_linked_paths() {
        let f = Fixture::new(1);
        let external = f.root.join("external");
        fs::create_dir(&external).unwrap();
        fs::create_dir(f.home.join(".gemini")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, f.home.join(".gemini/config")).unwrap();
        #[cfg(windows)]
        {
            // The fixture uses a verbatim canonical path. Give the Windows shell
            // absolute DOS paths instead of asking cmd to resolve a relative junction
            // from a verbatim working directory.
            let root = env::temp_dir().join(f.root.file_name().unwrap());
            let powershell = PathBuf::from(env::var_os("SystemRoot").unwrap())
                .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
            let output = std::process::Command::new(powershell)
                .env("DTW_TEST_LINK", root.join(r"home\.gemini\config"))
                .env("DTW_TEST_TARGET", root.join("external"))
                .args(["-NoProfile", "-NonInteractive", "-Command",
                    "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:DTW_TEST_LINK -Target $env:DTW_TEST_TARGET | Out-Null"])
                .output()
                .unwrap();
            assert!(output.status.success(), "{:?}", output);
            assert_eq!(
                f.home.join(r".gemini\config").canonicalize().unwrap(),
                external
            );
        }
        let result = agy(&f.package, &f.install, None, Mode::New, &f.home, None);
        assert_eq!(result["state"], "host_action_required");
        assert_eq!(fs::read_dir(&external).unwrap().count(), 0);
    }

    #[test]
    fn agy_app_clean_copy_pauses_for_enable_and_current_never_copies() {
        let f = Fixture::new(1);
        let result = agy(&f.package, &f.install, None, Mode::Current, &f.home, None);
        assert_eq!(result["state"], "host_action_required");
        assert!(!f.slot().exists());
        let result = agy(&f.package, &f.install, None, Mode::New, &f.home, None);
        assert_eq!(result["state"], "host_action_required");
        assert!(result["next"].as_str().unwrap().starts_with(APP_ENABLE));
        assert!(same_copy(&f.slot(), &f.install));
    }

    #[test]
    fn agy_app_enabled_matching_copy_returns_verify_and_disabled_pauses() {
        let f = Fixture::new(2);
        f.copy(&f.slot());
        f.set_app_enabled(true);
        let enabled = agy(&f.package, &f.install, None, Mode::Resume, &f.home, None);
        assert_eq!(enabled["state"], "verify");

        f.set_app_enabled(false);
        let disabled = agy(&f.package, &f.install, None, Mode::Resume, &f.home, None);
        assert_eq!(disabled["state"], "host_action_required");
        assert!(disabled["next"].as_str().unwrap().starts_with(APP_ENABLE));
    }

    #[cfg(unix)]
    #[test]
    fn agy_app_replaces_verified_previous_copy_and_requests_single_reload() {
        let f = Fixture::with_capability(2, Some(json!(1)));
        let (new_pkg, new_install) = f.add_package("new", 2, "0.6.2");
        f.copy(&f.slot());
        f.set_app_enabled(true);

        let result = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::New,
            &f.home,
            None,
        );
        assert_eq!(result["state"], "host_action_required");
        assert!(result["next"].as_str().unwrap().starts_with(APP_RELOAD_MCP));
        assert!(same_copy(&f.slot(), &new_install));
        let backup = Path::new(result["backup"].as_str().unwrap());
        assert!(same_copy(backup, &f.install));
        assert!(backup.starts_with(f.home.join(".gemini/config")));
        assert!(!backup.starts_with(f.slot().parent().unwrap()));
        assert_eq!(fs::read_dir(f.slot().parent().unwrap()).unwrap().count(), 1);

        let resumed = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::Resume,
            &f.home,
            None,
        );
        assert_eq!(resumed["state"], "verify");
    }

    #[cfg(windows)]
    #[test]
    fn agy_windows_never_auto_replaces_occupied_slot() {
        let f = Fixture::with_capability(2, Some(json!(1)));
        let (new_pkg, new_install) = f.add_package("new", 2, "0.6.2");
        f.copy(&f.slot());
        f.set_app_enabled(true);

        let result = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::New,
            &f.home,
            None,
        );
        assert_stopped_replacement(&result);
        let cli = |args: &[&str]| {
            assert_eq!(args, AGY_LIST);
            output(json!([{"name": ID, "path": f.slot()}]))
        };
        let result = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::New,
            &f.home,
            Some(&cli),
        );
        assert_stopped_replacement(&result);
        assert!(same_copy(&f.slot(), &f.install));
    }

    #[test]
    fn agy_app_never_replaces_unverified_or_mismatched_previous_copy() {
        let f = Fixture::with_capability(2, Some(json!(1)));
        let (new_pkg, new_install) = f.add_package("new", 2, "0.6.2");
        let (_, other_install) = f.add_package("other", 2, "0.5.9");
        f.copy(&f.slot());
        f.set_app_enabled(true);

        let current = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::Current,
            &f.home,
            None,
        );
        assert_eq!(current["state"], "host_action_required");
        assert!(same_copy(&f.slot(), &f.install));

        let mismatched = agy(
            &new_pkg,
            &new_install,
            Some(&other_install),
            Mode::New,
            &f.home,
            None,
        );
        assert_eq!(mismatched["state"], "host_action_required");
        assert!(same_copy(&f.slot(), &f.install));

        fs::write(f.slot().join("skills/dtw/SKILL.md"), "local edit").unwrap();
        let edited = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::New,
            &f.home,
            None,
        );
        assert_eq!(edited["state"], "host_action_required");
        assert_eq!(
            fs::read_to_string(f.slot().join("skills/dtw/SKILL.md")).unwrap(),
            "local edit"
        );
    }

    #[test]
    fn agy_app_never_replaces_rehashed_custom_copy_without_previous_pin() {
        let f = Fixture::new(2);
        let (new_pkg, new_install) = f.add_package("new", 2, "0.6.2");
        let slot = f.slot();
        fs::create_dir_all(slot.parent().unwrap()).unwrap();
        let custom_install = Fixture::build_package(&slot, 2, "0.6.0", None);
        assert_eq!(custom_install["package_id"], f.install["package_id"]);
        // Modify Skill and recompute valid BUILD.json + SHA256SUMS to model a custom/forked build.
        fs::write(slot.join("skills/dtw/SKILL.md"), "custom fork skill").unwrap();
        let mut build = files::json(&slot.join("BUILD.json")).unwrap();
        build["skill_sha256"] = json!(files::digest(&slot.join("skills/dtw/SKILL.md")).unwrap());
        fs::write(slot.join("BUILD.json"), serde_json::to_vec(&build).unwrap()).unwrap();
        let sums: String = files::inventory(&slot)
            .unwrap()
            .into_iter()
            .filter(|(name, _)| name != "SHA256SUMS")
            .map(|(name, path)| format!("{}  {name}\n", files::digest(&path).unwrap()))
            .collect();
        fs::write(slot.join("SHA256SUMS"), sums).unwrap();
        assert!(files::verify(&slot).is_ok());
        f.set_app_enabled(true);

        let result = agy(&new_pkg, &new_install, None, Mode::New, &f.home, None);
        assert_eq!(result["state"], "host_action_required");
        assert_eq!(
            fs::read_to_string(slot.join("skills/dtw/SKILL.md")).unwrap(),
            "custom fork skill"
        );
    }

    #[cfg(unix)]
    #[test]
    fn agy_app_replaces_verified_previous_copy_when_disabled_and_pauses_for_enable() {
        let f = Fixture::with_capability(2, Some(json!(1)));
        let (new_pkg, new_install) = f.add_package("new", 2, "0.6.2");
        f.copy(&f.slot());
        f.set_app_enabled(false);

        let result = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::New,
            &f.home,
            None,
        );
        assert_eq!(result["state"], "host_action_required");
        assert!(result["next"].as_str().unwrap().starts_with(APP_ENABLE));
        assert!(same_copy(&f.slot(), &new_install));
    }

    #[cfg(unix)]
    #[test]
    fn agy_cli_replaces_verified_previous_copy_and_returns_verify() {
        let f = Fixture::with_capability(2, Some(json!(1)));
        let (new_pkg, new_install) = f.add_package("new", 2, "0.6.2");
        let slot = f.slot();
        f.copy(&slot);
        let invisible = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::New,
            &f.home,
            Some(&|_| output(json!([]))),
        );
        assert_eq!(invisible["state"], "host_action_required");
        assert!(same_copy(&slot, &f.install));

        let calls = RefCell::new(Vec::new());
        let run = |args: &[&str]| {
            calls
                .borrow_mut()
                .push(args[0..args.len().min(2)].join(" "));
            match args {
                AGY_LIST => {
                    let version = if same_copy(&slot, &new_install) {
                        "0.6.2"
                    } else {
                        "0.6.0"
                    };
                    output(
                        json!([{"name":ID,"path":slot,"enabled":true,"version":version,"skills":["dtw"],"mcpServers":["dtw"]}]),
                    )
                }
                ["--version"] => Some(b"1.5.15\n".to_vec()),
                ["plugin", "validate", _, "--json"] => output(json!({
                    "dir":new_pkg,"name":ID,"manifest":".antigravity-plugin/plugin.json",
                    "components":{"skills":1,"mcpServers":1},
                    "findings":[{"severity":"warning","file":".mcp.json","message":AGY_WARNING}]
                })),
                _ => panic!("unexpected {args:?}"),
            }
        };
        let result = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::New,
            &f.home,
            Some(&run),
        );
        assert_eq!(result["state"], "verify");
        assert!(same_copy(&slot, &new_install));
        assert_eq!(
            *calls.borrow(),
            ["plugin list", "--version", "plugin validate", "plugin list"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn agy_live_swap_requires_the_integer_capability_in_both_verified_builds() {
        for (prior_capability, desired_capability) in [
            (None, Some(json!(1))),
            (Some(json!(1)), None),
            (Some(json!(true)), Some(json!(1))),
            (Some(json!(1)), Some(json!("1"))),
        ] {
            let f = Fixture::new(2);
            let prior_package = f.root.join("managed/prior");
            let prior = Fixture::build_package(&prior_package, 2, "0.6.1", prior_capability);
            let desired_package = f.root.join("managed/desired");
            let desired = Fixture::build_package(&desired_package, 2, "99.0.0", desired_capability);
            let slot = f.slot();
            f.set_app_enabled(true);
            fs::create_dir(slot.parent().unwrap()).unwrap();
            files::copy(
                &prior_package,
                &slot,
                &files::verify(&prior_package).unwrap(),
            )
            .unwrap();
            let result = agy(
                &desired_package,
                &desired,
                Some(&prior),
                Mode::New,
                &f.home,
                None,
            );
            assert_stopped_replacement(&result);
            assert!(same_copy(&slot, &prior));

            let backup = f.home.join(".gemini/config/manual-backup");
            rename_without_replace(&slot, &backup).unwrap();
            files::copy(
                &desired_package,
                &slot,
                &files::verify(&desired_package).unwrap(),
            )
            .unwrap();
            let resumed = agy(
                &desired_package,
                &desired,
                Some(&prior),
                Mode::Resume,
                &f.home,
                None,
            );
            assert_eq!(resumed["state"], "verify");
            assert!(same_copy(&backup, &prior));
        }
    }

    #[cfg(unix)]
    #[test]
    fn agy_cli_validation_failure_guides_stopped_replacement_of_the_occupied_slot() {
        let f = Fixture::with_capability(2, Some(json!(1)));
        let (new_pkg, new_install) = f.add_package("new", 2, "0.6.2");
        let slot = f.slot();
        f.copy(&slot);
        let run = |args: &[&str]| match args {
            AGY_LIST => output(json!([{"name": ID, "path": slot}])),
            ["--version"] => Some(b"1.5.15\n".to_vec()),
            ["plugin", "validate", _, "--json"] => {
                output(json!({"findings": [{"severity": "error"}]}))
            }
            _ => panic!("unexpected {args:?}"),
        };
        let result = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::New,
            &f.home,
            Some(&run),
        );
        assert_stopped_replacement(&result);
        assert!(result["next"]
            .as_str()
            .unwrap()
            .contains("Proceed only if the loader accepts both components"));
        assert!(same_copy(&slot, &f.install));
    }

    #[test]
    fn agy_rejects_extra_empty_directories_in_a_pinned_copy() {
        let f = Fixture::with_capability(2, Some(json!(1)));
        let (new_pkg, new_install) = f.add_package("new", 2, "0.6.2");
        let slot = f.slot();
        f.copy(&slot);
        f.set_app_enabled(true);
        let empty = slot.join("bin/local/empty");
        fs::create_dir_all(&empty).unwrap();
        let current = agy(&f.package, &f.install, None, Mode::Current, &f.home, None);
        assert_stopped_replacement(&current);
        let upgrade = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::New,
            &f.home,
            None,
        );
        assert_stopped_replacement(&upgrade);
        assert!(same_copy(&slot, &f.install));
        assert!(empty.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn agy_rejects_native_aliases_non_unicode_names_and_local_mode_changes() {
        #[cfg(target_os = "linux")]
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;

        let f = Fixture::with_capability(2, Some(json!(1)));
        let slot = f.slot();
        f.copy(&slot);
        f.set_app_enabled(true);
        for extra in [
            std::ffi::OsStr::new(r"bin\dtw"),
            #[cfg(target_os = "linux")]
            std::ffi::OsStr::from_bytes(b"local-\xff"),
        ] {
            let path = slot.join(extra);
            fs::write(&path, "user content")
                .unwrap_or_else(|error| panic!("create {extra:?}: {error}"));
            assert!(files::verify(&slot).is_err());
            assert_stopped_replacement(&agy(
                &f.package,
                &f.install,
                None,
                Mode::Current,
                &f.home,
                None,
            ));
            assert_eq!(fs::read_to_string(&path).unwrap(), "user content");
            fs::remove_file(path).unwrap();
        }
        let binary = slot.join("bin/dtw");
        let mode = fs::metadata(&binary).unwrap().permissions().mode() ^ 0o100;
        fs::set_permissions(&binary, fs::Permissions::from_mode(mode)).unwrap();
        assert!(same_copy(&slot, &f.install));
        assert_stopped_replacement(&agy(
            &f.package,
            &f.install,
            None,
            Mode::Current,
            &f.home,
            None,
        ));
        assert_eq!(fs::metadata(binary).unwrap().permissions().mode(), mode);
    }

    #[cfg(unix)]
    #[test]
    fn agy_unix_swap_preserves_a_concurrent_slot_edit_outside_discovery() {
        let f = Fixture::new(2);
        let (new_pkg, new_install) = f.add_package("new", 2, "0.6.2");
        let slot = f.slot();
        f.copy(&slot);
        let config = f.home.join(".gemini/config");
        let plugins = config.join("plugins");
        let source = files::verify(&new_pkg).unwrap();
        let prior = files::verify(&f.package).unwrap();
        let result = replace_copy_with(&new_pkg, &source, &prior, &config, &slot, |from, to| {
            assert!(from.starts_with(&config));
            assert!(!from.starts_with(&plugins));
            fs::write(to.join("skills/dtw/SKILL.md"), "concurrent edit").unwrap();
            swap_directory(from, to)
        });
        let Err(ReplaceError::Preserved(preserved)) = result else {
            panic!("expected Preserved error, got {result:?}");
        };
        assert!(preserved.starts_with(&config));
        assert!(!preserved.starts_with(&plugins));
        assert_eq!(
            fs::read_to_string(preserved.join("skills/dtw/SKILL.md")).unwrap(),
            "concurrent edit"
        );
        assert!(same_copy(&slot, &new_install));
        assert_eq!(fs::read_dir(&plugins).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn agy_unix_swap_leaves_an_occupant_published_after_the_first_exchange_in_place() {
        let f = Fixture::new(2);
        let (new_pkg, new_install) = f.add_package("new", 2, "0.6.2");
        let slot = f.slot();
        f.copy(&slot);
        let config = f.home.join(".gemini/config");
        let plugins = config.join("plugins");
        let source = files::verify(&new_pkg).unwrap();
        let prior = files::verify(&f.package).unwrap();
        let moved = config.join("concurrent-move");
        let result = replace_copy_with(&new_pkg, &source, &prior, &config, &slot, |from, to| {
            swap_directory(from, to)?;
            fs::rename(to, &moved)?;
            fs::create_dir(to)?;
            fs::write(to.join("foreign"), "new occupant")
        });
        let Err(ReplaceError::Preserved(preserved)) = result else {
            panic!("expected Preserved error, got {result:?}");
        };
        assert!(!preserved.starts_with(&plugins));
        assert!(same_copy(&preserved, &f.install));
        assert!(same_copy(&moved, &new_install));
        assert_eq!(
            fs::read_to_string(slot.join("foreign")).unwrap(),
            "new occupant"
        );
        assert_eq!(fs::read_dir(&plugins).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn agy_unix_swap_preserves_slot_when_initial_swap_fails() {
        let f = Fixture::new(2);
        let (new_pkg, _) = f.add_package("new", 2, "0.6.2");
        let slot = f.slot();
        f.copy(&slot);
        let config = f.home.join(".gemini/config");
        let plugins = config.join("plugins");
        let source = files::verify(&new_pkg).unwrap();
        let prior = files::verify(&f.package).unwrap();
        let result = replace_copy_with(&new_pkg, &source, &prior, &config, &slot, |_, _| {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        });
        assert_eq!(result, Err(ReplaceError::Aborted));
        assert!(same_copy(&slot, &f.install));
        assert_eq!(fs::read_dir(&plugins).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn agy_unix_swap_waits_for_the_shared_slot_lock() {
        let f = Fixture::with_capability(2, Some(json!(1)));
        let (new_pkg, new_install) = f.add_package("new", 2, "0.6.2");
        let slot = f.slot();
        f.copy(&slot);
        let config = f.home.join(".gemini/config");
        let lock = files::open_lock_file(&config.join(".dtw-setup.lock")).unwrap();
        lock.try_lock().unwrap();

        let receipt = agy(
            &new_pkg,
            &new_install,
            Some(&f.install),
            Mode::New,
            &f.home,
            None,
        );
        assert_eq!(receipt["state"], "host_action_required");
        assert!(receipt["next"]
            .as_str()
            .unwrap()
            .contains("Another setup is updating"));
        assert!(same_copy(&slot, &f.install));
        assert_eq!(fs::read_dir(config).unwrap().count(), 2);
    }

    #[test]
    fn grok_clean_install_stops_at_the_trust_decision() {
        let f = Fixture::new(2);
        let result = grok(&f.package, &f.install, Mode::New, &f.home, &|args| {
            assert_eq!(args, ["plugin", "list", "--json"]);
            output(json!([]))
        });
        assert_eq!(result["state"], "host_action_required");
        assert!(result["next"].as_str().unwrap().contains(&format!(
            "grok plugin install {} --trust",
            shell_path(&f.package)
        )));
        assert!(!f.home.join(".grok").exists());
    }

    #[test]
    fn grok_checks_copy_bytes_source_and_discovered_components() {
        let f = Fixture::new(2);
        let key = "payload-12345678";
        let copy = f.home.join(".grok/installed-plugins").join(key);
        f.copy(&copy);
        let list = json!([{"name":ID,"version":"0.6.0","repo_key":key,"path":copy,"source":f.package,"status":"installed"}]);
        let inspect = json!({
            "plugins":[{"name":ID,"scope":"user","path":copy,"enabled":true}],
            "skills":[{"name":"dtw","source":{"type":"plugin","plugin_name":ID,"path":copy.join("skills/dtw/SKILL.md")}}],
            "mcpServers":[{"name":"dtw","transport":"stdio","target":copy.join("bin/dtw"),"source":{"type":"plugin","plugin_name":ID,"path":copy}}]
        });
        let result = |list: &Value, inspect: &Value| {
            grok(
                &f.package,
                &f.install,
                Mode::Current,
                &f.home,
                &|args| match args {
                    ["plugin", "list", "--json"] => output(list.clone()),
                    ["inspect", "--json"] => output(inspect.clone()),
                    _ => panic!("unexpected {args:?}"),
                },
            )
        };
        assert_eq!(result(&list, &inspect)["state"], "verify");
        let mut foreign = list.clone();
        foreign[0]["source"] = json!(f.root.join("another-managed-package"));
        assert_eq!(result(&foreign, &inspect)["state"], "host_action_required");
        let mut blocked = inspect.clone();
        blocked["mcpServers"] = json!([]);
        assert_eq!(result(&list, &blocked)["state"], "host_action_required");
        fs::write(copy.join("bin/dtw"), "different binary").unwrap();
        assert_eq!(result(&list, &inspect)["state"], "host_action_required");
    }
}

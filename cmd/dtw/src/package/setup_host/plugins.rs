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
const APP_ENABLE: &str = "In Antigravity App, open Customizations → Installed and enable Desktop World with its bundled Skill and MCP.";

pub(super) fn register_agy_cli(package: &Path, install: &Value, mode: Mode) -> Value {
    let Some(home) = default_home(AGY_OVERRIDES) else {
        return agy_scope_action(package, false);
    };
    if !agy_scope_clear(&home) {
        return agy_scope_action(package, false);
    }
    let Some(cli) = HostCli::find("agy") else {
        return action("Put the installed agy CLI on an absolute PATH outside the workspace, then rerun setup or run `agy plugin list --json` in the intended profile to inspect Desktop World.");
    };
    agy(package, install, mode, &home, Some(&|args| cli.run(args)))
}

pub(super) fn register_agy_app(package: &Path, install: &Value, mode: Mode) -> Value {
    let Some(home) = default_home(AGY_OVERRIDES) else {
        return agy_scope_action(package, true);
    };
    if !agy_scope_clear(&home) {
        return agy_scope_action(package, true);
    }
    agy(package, install, mode, &home, None)
}

type RunCli<'a> = &'a dyn Fn(&[&str]) -> Option<Vec<u8>>;

fn agy(package: &Path, install: &Value, mode: Mode, home: &Path, cli: Option<RunCli<'_>>) -> Value {
    let app = cli.is_none();
    let config = home.join(".gemini/config");
    let plugins = config.join("plugins");
    let slot = plugins.join(ID);
    let slot_action = || {
        action(&format!("Inspect the Desktop World registration and the fixed slot {}. If the slot is empty, copy the complete package from {}. For a confirmed older DTW copy, stop its old connections before replacing the entire slot. Preserve unrelated files and rerun setup.", slot.display(), package.display()))
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
    let mut rows = if cli.is_some() {
        let Some(rows) = inventory() else {
            return action("Run `agy plugin list --json` in the intended profile and inspect the Desktop World entry before changing the shared plugin slot.");
        };
        rows
    } else {
        Vec::new()
    };
    match absent(&slot) {
        Ok(false) => {}
        Ok(true) if !matches!(single_plugin(&rows), Some(None)) => return slot_action(),
        Ok(true) if mode == Mode::Current => {
            return action(&format!("Check the intended Antigravity registration. If the fixed slot {} is truly empty, restore the complete package from {} and rerun setup.", slot.display(), package.display()));
        }
        Ok(true) => {
            if let Some(run) = cli {
                let Some(path) = package.to_str() else {
                    return agy_scope_action(package, false);
                };
                if !run(&["--version"])
                    .is_some_and(|bytes| String::from_utf8_lossy(&bytes).trim() == "1.5.15")
                    || !run(&["plugin", "validate", path, "--json"])
                        .is_some_and(|bytes| agy_validated(&bytes, package, manifest))
                {
                    return action(&format!("Run `agy plugin validate {} --json` in the intended CLI profile and review its Skill and MCP findings. If the loader accepts both components, place the complete package in the empty slot {}.", shell_path(package), slot.display()));
                }
            }
            // agy 1.5.15's installer can overwrite an occupied slot absent from
            // inventory. The shared App discovery path supports a no-replace copy.
            if owned_dirs(home, AGY_PLUGINS, true).is_err()
                || publish_copy(package, &source, &plugins, &slot).is_err()
            {
                return slot_action();
            }
            if cli.is_some() {
                let Some(after) = inventory() else {
                    return action("Run `agy plugin list --json` to confirm the installed Desktop World copy and its Skill and MCP, then rerun setup.");
                };
                rows = after;
            }
        }
        Err(_) => return slot_action(),
    }
    if !same_copy(&slot, install) {
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
        if cfg!(windows) && settings["plugins"][ID]["enabled"] == true {
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

fn publish_copy(
    package: &Path,
    source: &files::Verified,
    plugins: &Path,
    slot: &Path,
) -> io::Result<()> {
    // The staging wrapper has no manifest and the host only sees a complete slot.
    let stage = plugins.join(format!(".dtw-setup-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&stage)?;
    let payload = stage.join(ID);
    let result = files::copy(package, &payload, source)
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
        .and_then(|()| rename_without_replace(&payload, slot));
    let _ = fs::remove_dir_all(&stage);
    result
}

#[cfg(unix)]
fn rename_without_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // Both paths are NUL-terminated and live for the duration of this call.
    #[cfg(target_os = "macos")]
    let status = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let status = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
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
            let root = env::temp_dir().join(format!("dtw-plugin-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let root = fs::canonicalize(root).unwrap();
            let home = root.join("home");
            fs::create_dir(&home).unwrap();
            let package = root.join("managed/package");
            let service = if schema == 1 { ID } else { "dtw" };
            let skill = package.join("skills").join(service);
            fs::create_dir_all(&skill).unwrap();
            fs::create_dir(package.join("bin")).unwrap();
            let binary = package.join("bin/dtw");
            fs::write(&binary, "fixture binary").unwrap();
            for name in ["SKILL.md", "API.md", "PATTERNS.md"] {
                fs::write(skill.join(name), name).unwrap();
            }
            for name in [
                "plugin.json",
                "package.json",
                ".antigravity-plugin/plugin.json",
                ".claude-plugin/plugin.json",
            ] {
                let manifest = package.join(name);
                fs::create_dir_all(manifest.parent().unwrap()).unwrap();
                files::write(&manifest, &json!({"name":ID,"version":"0.6.0"})).unwrap();
            }
            files::write(
                &package.join("BUILD.json"),
                &json!({
                    "package_store_schema":schema,"version":"0.6.0","target":"aarch64-apple-darwin",
                    "binary":{"path":"bin/dtw","sha256":files::digest(&binary).unwrap()},
                    "skill_sha256":files::digest(&skill.join("SKILL.md")).unwrap()
                }),
            )
            .unwrap();
            let sums: String = files::inventory(&package)
                .unwrap()
                .iter()
                .map(|(name, path)| format!("{}  {name}\n", files::digest(path).unwrap()))
                .collect();
            fs::write(package.join("SHA256SUMS"), sums).unwrap();
            let id = files::verify(&package).unwrap().id;
            Self {
                root,
                home,
                package,
                install: json!({"package_id":id,"version":"0.6.0"}),
            }
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
        let result = agy(&f.package, &f.install, Mode::New, &f.home, Some(&run));
        assert_eq!(result["state"], "verify");
        assert!(same_copy(&slot, &f.install));
        assert_eq!(
            *calls.borrow(),
            ["plugin list", "--version", "plugin validate", "plugin list"]
        );
        calls.borrow_mut().clear();
        let result = agy(&f.package, &f.install, Mode::Current, &f.home, Some(&run));
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
            let result = agy(&f.package, &f.install, mode, &f.home, Some(&run));
            assert_eq!(result["state"], "host_action_required");
            assert_eq!(
                fs::read_to_string(slot.join("foreign")).unwrap(),
                "preserve"
            );
        }
        let source = files::verify(&f.package).unwrap();
        assert!(publish_copy(&f.package, &source, slot.parent().unwrap(), &slot).is_err());
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
        let result = agy(&f.package, &f.install, Mode::New, &f.home, None);
        assert_eq!(result["state"], "host_action_required");
        assert_eq!(fs::read_dir(&external).unwrap().count(), 0);
    }

    #[test]
    fn agy_app_clean_copy_pauses_for_enable_and_current_never_copies() {
        let f = Fixture::new(1);
        let result = agy(&f.package, &f.install, Mode::Current, &f.home, None);
        assert_eq!(result["state"], "host_action_required");
        assert!(!f.slot().exists());
        let result = agy(&f.package, &f.install, Mode::New, &f.home, None);
        assert_eq!(result["state"], "host_action_required");
        assert!(result["next"].as_str().unwrap().starts_with(APP_ENABLE));
        assert!(same_copy(&f.slot(), &f.install));
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

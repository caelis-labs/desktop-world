use super::{action, home, package_path, shell_path, variable_set, verify, Mode};
use serde_json::Value;
use std::{
    env,
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub(super) struct HostCli {
    executable: PathBuf,
    directory: PathBuf,
    search_path: OsString,
}

impl HostCli {
    pub(super) fn find(name: &str) -> Option<Self> {
        let directory = fs::canonicalize(home()?).ok()?;
        let cwd = fs::canonicalize(env::current_dir().ok()?).ok()?;
        Self::find_from(name, directory, &cwd, &env::var_os("PATH")?)
    }

    fn find_from(
        name: &str,
        directory: PathBuf,
        cwd: &Path,
        path: &std::ffi::OsStr,
    ) -> Option<Self> {
        let workspace = checkout(cwd).or_else(|| (cwd != directory).then_some(cwd));
        let filename = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        };
        let is_project = |path: &Path| {
            workspace.is_some_and(|root| path.starts_with(root)) || checkout(path).is_some()
        };
        let mut search = Vec::new();
        let mut found = None;
        for entry in env::split_paths(path) {
            if !entry.is_absolute() {
                continue;
            }
            if fs::canonicalize(&entry)
                .is_ok_and(|resolved| !is_project(&resolved) && !is_project(&entry))
            {
                search.push(entry.clone());
            }
            if found.is_some() {
                continue;
            }
            let candidate = entry.join(&filename);
            let Ok(executable) = fs::canonicalize(&candidate) else {
                continue;
            };
            let Ok(metadata) = fs::metadata(&executable) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            // Do not run a tool from the invoking workspace or any Git checkout,
            // even when a PATH entry or executable symlink points into it.
            if is_project(&executable) || is_project(&candidate) {
                return None;
            }
            found = Some(executable);
        }
        Some(Self {
            executable: found?,
            directory,
            search_path: env::join_paths(search).ok()?,
        })
    }

    pub(super) fn run(&self, args: &[&str]) -> Option<Vec<u8>> {
        self.run_bounded(args, Duration::from_secs(20))
    }

    fn run_bounded(&self, args: &[&str], timeout: Duration) -> Option<Vec<u8>> {
        const MAX_OUTPUT: usize = 2 * 1024 * 1024;
        let mut child = Command::new(&self.executable)
            .args(args)
            .current_dir(&self.directory)
            .env("PATH", &self.search_path)
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let (send, receive) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let read = stdout.take(MAX_OUTPUT as u64 + 1).read_to_end(&mut bytes);
            let _ = send.send((read.is_ok() && bytes.len() <= MAX_OUTPUT).then_some(bytes));
        });
        let deadline = Instant::now() + timeout;
        let mut output = None;
        let mut finished = false;
        while Instant::now() < deadline {
            if !finished {
                match child.try_wait() {
                    Ok(Some(status)) if status.success() => finished = true,
                    Ok(Some(_)) => return None,
                    Ok(None) => (),
                    Err(_) => break,
                }
            }
            if output.is_none() {
                match receive.recv_timeout(Duration::from_millis(10)) {
                    Ok(Some(bytes)) => output = Some(bytes),
                    Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                }
            } else if !finished {
                thread::sleep(Duration::from_millis(10));
            }
            if finished && output.is_some() {
                return output;
            }
        }
        if !finished {
            // This handle refers only to the directly spawned headless command.
            // No host app, daemon, process group or existing session is stopped.
            let _ = child.kill();
            let _ = child.wait();
        }
        None
    }
}

fn checkout(path: &Path) -> Option<&Path> {
    path.ancestors().find(|p| p.join(".git").exists())
}

pub(super) fn register_caelis(
    package: &Path,
    install: &Value,
    previous: Option<&Value>,
    mode: Mode,
) -> Value {
    if [
        "CAELIS_STORE_DIR",
        "CAELIS_CONTROL_URL",
        "CAELIS_CONTROL_EMBEDDED",
        "CAELIS_CONTROL_LISTEN",
        "CAELIS_CONTROL_TOKEN",
        "CAELIS_CONTROL_TOKEN_FILE",
    ]
    .iter()
    .any(|name| variable_set(name))
    {
        return action("Use Caelis's plugin manager in the intended local Host and Store, then start a new Session.");
    }
    let Some(cli) = HostCli::find("caelis") else {
        return action("Put the installed Caelis CLI on an absolute PATH outside the workspace, then rerun setup.");
    };
    let version = cli
        .run(&["version", "--format", "json"])
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if !version.is_some_and(|version| version["build_kind"] == "release") {
        return action("Confirm you are using an installed Caelis release and its default local Store, then rerun setup or use the intended Host's plugin manager.");
    }
    caelis(package, install, previous, mode, |args| cli.run(args))
}

fn caelis(
    package: &Path,
    install: &Value,
    previous: Option<&Value>,
    mode: Mode,
    run: impl Fn(&[&str]) -> Option<Vec<u8>>,
) -> Value {
    // Managed CLI attachment can upgrade/restart an older running Host.
    // The documented embedded mode takes exclusive ownership or fails.
    const STATUS: &[&str] = &["plugin", "status", "desktop-world", "--json", "--embedded"];
    let status = || run(STATUS).and_then(|bytes| parse_caelis_status(&bytes));
    let Some(first) = status() else {
        if mode == Mode::Resume {
            return finish_caelis_install(package);
        }
        return action(&format!("If Caelis is running, use `/plugin install {}` in the intended local Host after checking its Desktop World registration. Otherwise confirm that your installed Caelis release supports `plugin status desktop-world --json --embedded` and rerun setup.", shell_path(package)));
    };
    let snapshot = match first {
        CaelisStatus::Registered(plugin) => plugin,
        CaelisStatus::Absent if mode == Mode::Current => {
            return action("Restore this managed package through the intended Caelis Host's plugin manager, then rerun setup.");
        }
        CaelisStatus::Absent if mode == Mode::Resume => return finish_caelis_install(package),
        CaelisStatus::Absent => {
            let Some(path) = package.to_str().filter(|path| path.trim() == *path) else {
                return action("Use Caelis's plugin manager to register the managed package path.");
            };
            // An unsuccessful command can still commit. Observe the Host again
            // before interpreting any outcome or offering another attempt.
            let _ = run(&["plugin", "install", path, "--json", "--embedded"]);
            match status() {
                Some(CaelisStatus::Registered(plugin)) => plugin,
                _ => return finish_caelis_install(package),
            }
        }
    };
    let selected = snapshot["root"]
        .as_str()
        .filter(|path| Path::new(path).is_absolute())
        .and_then(|path| fs::canonicalize(path).ok());
    if selected.is_none() || selected != fs::canonicalize(package).ok() {
        if let Some(prior) = previous
            .filter(|prior| prior["version"] == snapshot["version"])
            .and_then(|prior| package_path(prior).ok())
            .filter(|prior| selected.is_some() && selected == fs::canonicalize(prior).ok())
        {
            return action(&format!("Finish active DTW work in Caelis. Confirm `caelis plugin status desktop-world --json` still points to {}, then run `caelis plugin remove desktop-world --json` followed by `caelis plugin install {} --json`. Start a new Session and rerun setup.", shell_path(prior), shell_path(package)));
        }
        return action("Caelis already has a different Desktop World registration. Review its source in the plugin manager and switch to this managed package in the intended Host.");
    }
    if snapshot["enabled"] != true
        || snapshot["version"] != install["version"]
        || !matches!(snapshot["status"].as_str(), Some("active" | "inactive"))
    {
        return action("Inspect the exact Desktop World registration in Caelis, confirm the managed package version and enable it before starting a new Session.");
    }
    verify("Start a new Caelis Session and verify the live DTW tools and matching bundled Skill.")
}

fn finish_caelis_install(package: &Path) -> Value {
    action(&format!("Wait for any prior Caelis install to finish before running another command. Check `caelis plugin status desktop-world --json --embedded` in the intended local Store. If it confirms Desktop World is still absent, run `caelis plugin install {} --json --embedded` once, then rerun setup.", shell_path(package)))
}

enum CaelisStatus {
    Absent,
    Registered(Value),
}

fn parse_caelis_status(bytes: &[u8]) -> Option<CaelisStatus> {
    let status: Value = serde_json::from_slice(bytes).ok()?;
    if status["schema_version"] != "1"
        || status["action"] != "status"
        || status["id"] != "desktop-world"
        || status.get("error").is_some()
    {
        return None;
    }
    match status["installed"].as_bool()? {
        false if status.get("plugin").is_none() => Some(CaelisStatus::Absent),
        true if status["plugin"]["id"] == "desktop-world" => {
            Some(CaelisStatus::Registered(status["plugin"].clone()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    #[cfg(unix)]
    #[test]
    fn host_lookup_rejects_workspace_tools_and_symlinks() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let home = fs::canonicalize(env::temp_dir())
            .unwrap()
            .join(uuid::Uuid::new_v4().to_string());
        let workspace = home.join("project");
        let outside = home.join("bin");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(workspace.join("pi"), "mock tool").unwrap();
        fs::set_permissions(workspace.join("pi"), fs::Permissions::from_mode(0o700)).unwrap();
        symlink(workspace.join("pi"), outside.join("pi")).unwrap();
        let search = env::join_paths([&workspace, &outside]).unwrap();
        assert!(HostCli::find_from("pi", home.clone(), &workspace, &search).is_none());
        assert!(HostCli::find_from("pi", home.clone(), &workspace, outside.as_os_str()).is_none());
        fs::remove_file(outside.join("pi")).unwrap();
        fs::write(outside.join("pi"), "mock tool").unwrap();
        fs::set_permissions(outside.join("pi"), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(HostCli::find_from("pi", home.clone(), &workspace, outside.as_os_str()).is_some());
        fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn command_runner_limits_elapsed_time_and_output() {
        let cli = HostCli {
            executable: PathBuf::from("/bin/sh"),
            directory: env::temp_dir(),
            search_path: OsString::from("/usr/bin:/bin"),
        };
        assert_eq!(
            cli.run(&["-c", "printf controlled"]),
            Some(b"controlled".to_vec())
        );
        assert!(cli
            .run(&["-c", "exec /usr/bin/head -c 2097153 /dev/zero"])
            .is_none());
        assert!(cli
            .run_bounded(&["-c", "exec /bin/sleep 2"], Duration::from_millis(30))
            .is_none());
    }

    #[test]
    fn caelis_reads_before_install_and_observes_committed_result() {
        let package = env::temp_dir();
        let root = fs::canonicalize(&package).unwrap();
        let calls = RefCell::new(Vec::new());
        let result = caelis(
            &package,
            &json!({"version":"1.0.0"}),
            None,
            Mode::New,
            |args| {
                let mut calls = calls.borrow_mut();
                calls.push(args[1].to_owned());
                let status = match calls.len() {
                    1 => {
                        json!({"schema_version":"1","action":"status","id":"desktop-world","installed":false})
                    }
                    2 => return None, // Failed exit after the Host committed.
                    _ => {
                        json!({"schema_version":"1","action":"status","id":"desktop-world","installed":true,
                    "plugin":{"id":"desktop-world","root":root,"enabled":true,"version":"1.0.0","status":"inactive"}})
                    }
                };
                Some(status.to_string().into_bytes())
            },
        );
        assert_eq!(result["state"], "verify");
        assert_eq!(*calls.borrow(), ["status", "install", "status"]);
    }

    #[test]
    fn caelis_resume_observes_absent_and_exact_registration_without_installing() {
        let package = fs::canonicalize(env::temp_dir()).unwrap();
        for installed in [false, true] {
            let result = caelis(
                &package,
                &json!({"version":"1.0.0"}),
                None,
                Mode::Resume,
                |args| {
                    assert_eq!(
                        args,
                        ["plugin", "status", "desktop-world", "--json", "--embedded"]
                    );
                    let mut status = json!({"schema_version":"1","action":"status","id":"desktop-world","installed":installed});
                    if installed {
                        status["plugin"] = json!({"id":"desktop-world","root":package,"version":"1.0.0","enabled":true,"status":"active"});
                    }
                    Some(status.to_string().into_bytes())
                },
            );
            if installed {
                assert_eq!(result["state"], "verify");
            } else {
                assert_eq!(result["state"], "host_action_required");
                let next = result["next"].as_str().unwrap();
                assert!(next.contains("Wait for any prior Caelis install to finish"));
                assert!(next.contains(&format!(
                    "caelis plugin install {} --json --embedded` once",
                    shell_path(&package)
                )));
            }
        }
    }

    #[test]
    fn caelis_uncertain_install_waits_for_completion_and_checks_before_retry() {
        let package = env::temp_dir();
        let calls = RefCell::new(Vec::new());
        let result = caelis(&package, &json!({}), None, Mode::New, |args| {
            let mut calls = calls.borrow_mut();
            calls.push(args[1].to_owned());
            (calls.len() == 1).then(|| json!({"schema_version":"1","action":"status","id":"desktop-world","installed":false}).to_string().into_bytes())
        });
        assert_eq!(*calls.borrow(), ["status", "install", "status"]);
        assert_eq!(result["state"], "host_action_required");
        let next = result["next"].as_str().unwrap();
        assert!(next.contains(
            "Wait for any prior Caelis install to finish before running another command"
        ));
        assert!(next.contains("If it confirms Desktop World is still absent"));
    }

    #[test]
    fn caelis_hands_off_an_exact_previous_registration() {
        let previous = fs::canonicalize(env::temp_dir()).unwrap();
        let package = previous.join("new managed package");
        let result = caelis(
            &package,
            &json!({"version":"2.0.0"}),
            Some(&json!({"package_path":previous,"version":"1.0.0"})),
            Mode::Resume,
            |args| {
                assert_eq!(
                    args,
                    ["plugin", "status", "desktop-world", "--json", "--embedded"]
                );
                let status = json!({"schema_version":"1","action":"status","id":"desktop-world","installed":true,
                    "plugin":{"id":"desktop-world","root":previous,"version":"1.0.0","enabled":true,"status":"inactive"}});
                Some(status.to_string().into_bytes())
            },
        );
        assert_eq!(result["state"], "host_action_required");
        let next = result["next"].as_str().unwrap();
        assert!(next.contains("caelis plugin remove desktop-world --json"));
        assert!(next.contains(&format!(
            "caelis plugin install {} --json",
            shell_path(&package)
        )));
    }

    #[test]
    fn caelis_current_and_ambiguous_status_never_install() {
        for (mode, status) in [
            (
                Mode::Current,
                json!({"schema_version":"1","action":"status","id":"desktop-world","installed":false}),
            ),
            (
                Mode::Resume,
                json!({"schema_version":"1","action":"status","id":"desktop-world","installed":true,
                "plugin":{"id":"desktop-world","root":"unrelated","enabled":true,"secret":"credential"}}),
            ),
            (Mode::New, json!({"installed":false,"error":"credential"})),
        ] {
            let result = caelis(&env::temp_dir(), &json!({}), None, mode, |args| {
                assert_eq!(
                    args,
                    ["plugin", "status", "desktop-world", "--json", "--embedded"]
                );
                Some(status.to_string().into_bytes())
            });
            assert_eq!(result["state"], "host_action_required");
            assert!(!result.to_string().contains("credential"));
        }
    }
}

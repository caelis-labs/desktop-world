//! Deliberately partial desktop-entry discovery and argument-free application launch.
//! X11 window close is not application quit; no quit operation is provided here.
use super::*;
use std::{
    collections::BTreeMap,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
};

struct Entry {
    name: String,
    executable: PathBuf,
}
struct Installed {
    entries: BTreeMap<String, Entry>,
    gaps: Vec<&'static str>,
}

fn desktop_entry(text: &str) -> Option<Entry> {
    let mut values = BTreeMap::new();
    let mut main = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            main = line == "[Desktop Entry]";
        } else if main && !line.is_empty() && !line.starts_with('#') {
            let (key, value) = line.split_once('=')?;
            if values.insert(key.trim(), value.trim()).is_some() {
                return None;
            }
        }
    }
    if values.get("Type") != Some(&"Application")
        || ["Hidden", "NoDisplay", "Terminal", "DBusActivatable"]
            .iter()
            .any(|key| values.get(key).is_some_and(|value| *value != "false"))
        || ["Path", "TryExec", "OnlyShowIn", "NotShowIn"]
            .iter()
            .any(|key| values.contains_key(key))
    {
        return None;
    }
    let name = values.get("Name")?;
    let exec = values.get("Exec")?;
    // Reject all quoting, escaping, field substitution and arguments, rather than
    // implementing a second command parser. Never accept a command from exec JS.
    if name.is_empty()
        || name.chars().any(char::is_control)
        || name.contains('\\')
        || !exec.starts_with('/')
        || !exec
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-.+".contains(&b))
    {
        return None;
    }
    let executable = std::fs::canonicalize(exec).ok()?;
    let metadata = std::fs::metadata(&executable).ok()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return None;
    }
    let mut header = [0; 4];
    File::open(&executable).ok()?.read_exact(&mut header).ok()?;
    if header != *b"\x7fELF" {
        return None;
    }
    Some(Entry {
        name: (*name).into(),
        executable,
    })
}

fn installed(until: Instant) -> Result<Installed, Error> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME").filter(|p| !p.is_empty()) {
        roots.push(PathBuf::from(home));
    } else if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share"));
    }
    let dirs = std::env::var_os("XDG_DATA_DIRS")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    roots.extend(std::env::split_paths(&dirs));
    let mut result = Installed {
        entries: BTreeMap::new(),
        gaps: vec![],
    };
    let mut seen = std::collections::HashSet::new();
    let mut count = 0;
    for root in roots {
        check(until)?;
        if !root.is_absolute() {
            result.gaps.push("relative_xdg_data_directory");
            continue;
        }
        let files = match std::fs::read_dir(root.join("applications")) {
            Ok(files) => files,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                result.gaps.push("desktop_catalog_unreadable");
                continue;
            }
        };
        for file in files {
            check(until)?;
            count += 1;
            if count > 4096 {
                result.gaps.push("desktop_catalog_scan_budget");
                return Ok(result);
            }
            let file = match file {
                Ok(file) => file,
                Err(_) => {
                    result.gaps.push("desktop_entry_unreadable");
                    continue;
                }
            };
            let path = file.path();
            if path.extension().is_none_or(|ext| ext != "desktop") {
                continue;
            }
            // First XDG directory wins, including hidden and unsupported entries.
            if !seen.insert(file.file_name()) {
                continue;
            }
            let text = match OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
                .and_then(|file| {
                    let m = file.metadata()?;
                    if !m.is_file() || m.len() > 65536 {
                        return Err(std::io::Error::other("unsupported desktop entry file"));
                    }
                    let mut text = String::new();
                    file.take(65537).read_to_string(&mut text)?;
                    if text.len() > 65536 {
                        return Err(std::io::Error::other("entry too large"));
                    }
                    Ok(text)
                }) {
                Ok(text) => text,
                Err(_) => {
                    result.gaps.push("desktop_entry_unreadable");
                    continue;
                }
            };
            if let Some(entry) = desktop_entry(&text) {
                result
                    .entries
                    .entry(entry.executable.to_string_lossy().into_owned())
                    .or_insert(entry);
            }
        }
    }
    result.gaps.sort();
    result.gaps.dedup();
    Ok(result)
}

impl Store {
    fn lifecycle_running(
        &mut self,
        until: Instant,
    ) -> Result<BTreeMap<String, (String, Vec<AppInstance>)>, Error> {
        let mut apps: BTreeMap<String, (String, Vec<AppInstance>)> = BTreeMap::new();
        // Preserve the existing running catalog's best-effort treatment of
        // unprovable foreign clients; they never supply a launch confirmation.
        for window in self.clients()? {
            check(until)?;
            if let Ok(pid) = self.owner(window) {
                if let Ok(app) = process_stamp(pid) {
                    let id = app.identity.0.clone();
                    let name = id.rsplit('/').next().unwrap_or(&id).to_owned();
                    let instances = &mut apps.entry(id).or_insert((name, vec![])).1;
                    if !instances.contains(&app) {
                        instances.push(app);
                    }
                }
            }
        }
        Ok(apps)
    }
    pub(super) fn discover_apps(&mut self, until: Instant) -> Result<Value, Error> {
        let installed = installed(until)?;
        let has_installed = !installed.entries.is_empty();
        let mut apps = self.lifecycle_running(until)?;
        for (id, entry) in installed.entries {
            apps.entry(id)
                .and_modify(|(name, _)| *name = entry.name.clone())
                .or_insert((entry.name, vec![]));
        }
        let rows: Vec<_> = apps
            .into_iter()
            .map(|(id, (name, instances))| {
                let mut row = json!({"app":id,"name":name,"running":!instances.is_empty()});
                if instances.len() > 1 {
                    row["instances"] = json!(instances.len());
                }
                row
            })
            .collect();
        Ok(
            json!({"apps":rows,"coverage":{"scope":if has_installed {"restricted_desktop_entries_and_running_x11"} else {"running_x11"},"installed":has_installed,"complete":false,"locations":["xdg_applications_top_level","running_local_x11_windows"],"read_error_codes":installed.gaps,"limitations":["installed entries require argument-free absolute ELF Exec; nested entries and other desktop-entry forms are excluded","running coverage requires XRes-proven local X11 windows; normal application quit is unsupported"]}}),
        )
    }
    pub(super) fn resolve_app(&mut self, r: &Value, until: Instant) -> Result<Value, Error> {
        let id = string(r, "app_id")?;
        let running = self.lifecycle_running(until)?;
        let installed = installed(until)?;
        let (name, instances) = if let Some((name, instances)) = running.get(id) {
            (
                installed
                    .entries
                    .get(id)
                    .map(|entry| entry.name.clone())
                    .unwrap_or_else(|| name.clone()),
                instances.clone(),
            )
        } else if let Some(entry) = installed.entries.get(id) {
            (entry.name.clone(), vec![])
        } else {
            return Err(Error::NativeFailure {
                code: "app_not_found".into(),
                message: "No matching running X11 application or eligible desktop entry".into(),
            });
        };
        Ok(
            json!({"app_id":id,"name":name,"apps":instances.iter().map(|app| json!({"app":app_json(app)})).collect::<Vec<_>>()}),
        )
    }
    pub(super) fn launch_app(&mut self, r: &Value, until: Instant) -> Value {
        let mut dispatched = false;
        let result = (|| {
            let _lease = Lease::acquire(until)?;
            let id = string(r, "app_id")?;
            let installed = installed(until)?;
            let entry = installed
                .entries
                .get(id)
                .ok_or(Error::UnsupportedCapability)?;
            let running = self.lifecycle_running(until)?;
            if let Some((_, apps)) = running.get(id) {
                return Ok(
                    json!({"dispatch":"not_dispatched","evidence":"confirmed","postcondition":"application_running","apps":apps.iter().map(|app|json!({"app":app_json(app)})).collect::<Vec<_>>()}),
                );
            }
            // A windowless instance cannot be safely reactivated by this subset.
            for process in std::fs::read_dir("/proc").map_err(failed)? {
                check(until)?;
                let process = process.map_err(failed)?;
                if process
                    .file_name()
                    .to_string_lossy()
                    .parse::<u32>()
                    .is_err()
                {
                    continue;
                }
                let metadata = match process.metadata() {
                    Ok(m) => m,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(failed(e)),
                };
                if metadata.uid() != unsafe { libc::geteuid() } {
                    continue;
                }
                match std::fs::read_link(process.path().join("exe")) {
                    Ok(path) if path == entry.executable => {
                        return Err(Error::UnsupportedCapability)
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(failed(e)),
                }
            }
            check(until)?;
            let mutation = worker::begin(until)?;
            // Spawn exactly once. The desktop application's lifetime is independent
            // of the metadata worker. Drain no pipes and reap its child on exit.
            let child = Command::new(&entry.executable)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(failed)?;
            dispatched = true;
            drop(mutation);
            let launched = process_stamp(child.id());
            std::thread::spawn(move || {
                let mut child = child;
                let _ = child.wait();
            });
            // Capture the incarnation before reaping can release this PID.
            let launched = launched?;
            let settle = until.min(Instant::now() + Duration::from_secs(5));
            loop {
                check(until)?;
                if worker::closing() {
                    return Err(Error::Closed);
                }
                let apps: Vec<_> = self
                    .lifecycle_running(until)?
                    .remove(id)
                    .map(|(_, apps)| apps)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|app| app == &launched)
                    .collect();
                if !apps.is_empty() || Instant::now() >= settle {
                    let confirmed = !apps.is_empty();
                    return Ok(
                        json!({"dispatch":"dispatched","evidence":if confirmed {"confirmed"} else {"unverified"},"postcondition":if confirmed {Some("application_running")} else {None},"apps":apps.iter().map(|app|json!({"app":app_json(app)})).collect::<Vec<_>>()}),
                    );
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        })();
        match result {
            Ok(value) => value,
            Err(error) => {
                let mut reply = error_reply(error, true);
                reply["apps"] = json!([]);
                if dispatched {
                    reply["dispatch"] = json!("unknown");
                }
                reply
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_command_parsing_and_non_application_entries() {
        let executable = std::fs::read_link("/proc/self/exe").unwrap();
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=Fixture\nExec={}\n",
            executable.display()
        );
        assert!(desktop_entry(&entry).is_some());
        for suffix in [
            " --flag",
            " %U",
            ";touch",
            "\\ argument",
            "\"",
            "\nTerminal=true",
            "\nHidden=true",
            "\nDBusActivatable=true",
            "\nPath=/tmp",
            "\nExec=/bin/true",
        ] {
            assert!(
                desktop_entry(&(entry.trim_end().to_owned() + suffix)).is_none(),
                "{suffix}"
            );
        }
        assert!(
            desktop_entry("[Desktop Entry]\nType=Link\nName=Fixture\nExec=/bin/true").is_none()
        );
    }
}

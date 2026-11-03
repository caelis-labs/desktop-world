//! Installer-owned immutable packages and per-host pins. No desktop tool surface.
mod files;
use files::*;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs::{self, File},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const SCHEMA: u64 = 1;
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 96
        || value.starts_with('.')
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b))
        || reserved_name(value)
    {
        return Err("Invalid host/profile identifier".into());
    }
    Ok(())
}
fn package_id(id: &str) -> Result<()> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Err("Invalid package identity".into())
    } else {
        Ok(())
    }
}
fn lock(path: &Path) -> Result<File> {
    let file = open_lock_file(path)?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            Err(error) => return Err(format!("Package store busy or lock unavailable: {error}")),
        }
    }
}
pub fn default_store() -> Result<PathBuf> {
    #[cfg(windows)]
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("DesktopWorld"));
    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|p| p.join("Library/Application Support/DesktopWorld"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|p| p.join(".local/share"))
        })
        .map(|p| p.join("desktop-world"));
    root.ok_or_else(|| "No default per-user package store; provide --store".into())
}
struct Store {
    root: PathBuf,
    _lock: File,
}
impl Store {
    fn open(root: &Path) -> Result<Self> {
        if !root.is_absolute() || root.parent().is_none() {
            return Err("Store must be an absolute dedicated directory".into());
        }
        if root.exists() {
            regular(root, true)?;
        } else {
            fs::create_dir_all(root).map_err(io_error)?;
        }
        let root = fs::canonicalize(root).map_err(io_error)?;
        if !root.join("STORE.json").exists() {
            // Claim only a new/empty directory; never adopt arbitrary user data.
            if fs::read_dir(&root).map_err(io_error)?.next().is_some() {
                return Err("Directory is not a managed DTW store".into());
            }
            write(
                &root.join("STORE.json"),
                &json!({"schema":SCHEMA,"kind":"dtw-package-store"}),
            )?;
        }
        regular(&root.join("STORE.json"), false)?;
        if json(&root.join("STORE.json"))? != json!({"schema":SCHEMA,"kind":"dtw-package-store"}) {
            return Err("Invalid store ownership".into());
        }
        let guard = lock(&root.join("store.lock"))?;
        for name in ["packages", "hosts", "leases", "staging", "trash"] {
            let p = root.join(name);
            if !p.exists() {
                fs::create_dir(&p).map_err(io_error)?;
            }
            regular(&p, true)?;
        }
        Ok(Self { root, _lock: guard })
    }
    fn package(&self, id: &str) -> Result<PathBuf> {
        package_id(id)?;
        let p = self.root.join("packages").join(id);
        regular(&p, true)?;
        regular(&p.join("PACKAGE.json"), false)?;
        let record = json(&p.join("PACKAGE.json"))?;
        if record["id"] != id || record["schema"] != SCHEMA {
            return Err("Managed package identity mismatch".into());
        }
        regular(&p.join("payload"), true)?;
        Ok(p)
    }
    fn recipe(&self, id: &str) -> Result<Value> {
        let p = self.package(id)?.join("payload");
        let verified = verify(&p)?;
        if verified.id != id {
            return Err("Managed package has changed".into());
        }
        let binary = p.join(verified.build["binary"]["path"].as_str().unwrap());
        Ok(
            json!({"package_id":id,"version":verified.build["version"],"target":verified.build["target"],"package_path":text(&p)?,"executable":text(&binary)?,"args":["mcp"],"skills_path":text(&p.join("skills"))?,"cordis_url":file_url(&p.join("cordis/index.mjs"))?,"agy_source":text(&p)?,"requires_fresh_host_context":true}),
        )
    }
    fn host(&self, name: &str) -> Result<PathBuf> {
        identifier(name)?;
        Ok(self.root.join("hosts").join(format!("{name}.json")))
    }
    fn update_host(&self, name: &str, value: &Value) -> Result<()> {
        let path = self.host(name)?;
        let temporary = self
            .root
            .join("staging")
            .join(format!("binding-{}.json", uuid::Uuid::new_v4()));
        write(&temporary, value)?;
        if path.exists() {
            regular(&path, false)?;
        }
        // Rust rename replaces a regular destination on both platforms. No old
        // file is removed first; an open Windows handle can refuse the swap.
        match fs::rename(&temporary, &path) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                Err(io_error(error))
            }
        }
    }

    fn bindings(&self) -> Result<Vec<Value>> {
        let mut rows = Vec::new();
        for entry in fs::read_dir(self.root.join("hosts")).map_err(io_error)? {
            let p = entry.map_err(io_error)?.path();
            if p.extension().is_some_and(|v| v == "json") {
                regular(&p, false)?;
                let row = json(&p)?;
                let host = p
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .ok_or("Invalid host name")?;
                identifier(host)?;
                if row["host"] != host {
                    return Err("Host binding identity mismatch".into());
                }
                for key in ["current", "pending", "rollback"] {
                    if !row[key].is_null() {
                        package_id(row[key].as_str().ok_or("Invalid host package pin")?)?;
                    }
                }
                rows.push(row);
            }
        }
        Ok(rows)
    }
    fn import(&self, source: &Path) -> Result<Value> {
        let source = fs::canonicalize(source).map_err(io_error)?;
        let verified = verify(&source)?;
        let id = &verified.id;
        let destination = self.root.join("packages").join(id);
        if !destination.exists() {
            let stage = self
                .root
                .join("staging")
                .join(uuid::Uuid::new_v4().to_string());
            fs::create_dir(&stage).map_err(io_error)?;
            write(
                &stage.join("STAGE.json"),
                &json!({"kind":"dtw-owned-stage","created_at":now(),"id":id}),
            )?;
            let copied = copy(&source, &stage.join("payload"), &verified);
            if let Err(error) = copied {
                let _ = fs::remove_dir_all(&stage);
                return Err(error);
            }
            write(
                &stage.join("PACKAGE.json"),
                &json!({"schema":SCHEMA,"id":id,"imported_at":now(),"version":verified.build["version"],"target":verified.build["target"]}),
            )?;
            fs::rename(&stage, &destination).map_err(io_error)?;
        }
        self.recipe(id)
    }
    fn bind(&self, name: &str, id: &str) -> Result<Value> {
        let recipe = self.recipe(id)?;
        let path = self.host(name)?;
        let mut row = if path.exists() {
            regular(&path, false)?;
            json(&path)?
        } else {
            json!({"host":name,"current":null,"rollback":null})
        };
        row["pending"] = json!(id);
        row["updated_at"] = json!(now());
        self.update_host(name, &row)?;
        Ok(
            json!({"binding":row,"install":recipe,"next":"Drain this host's old connections/contexts, configure this complete package, reconnect and verify actual tools/Skill; then activate this host pin. Other hosts stay pinned."}),
        )
    }
    fn activate(&self, name: &str) -> Result<Value> {
        let p = self.host(name)?;
        regular(&p, false)?;
        let mut row = json(&p)?;
        let id = row["pending"]
            .as_str()
            .ok_or("No pending host binding")?
            .to_owned();
        self.recipe(&id)?;
        if row["current"] != id {
            row["rollback"] = row["current"].clone();
        }
        row["current"] = json!(id);
        row["pending"] = Value::Null;
        row["updated_at"] = json!(now());
        self.update_host(name, &row)?;
        Ok(
            json!({"binding":row,"collection":self.gc(true,7).unwrap_or_else(|error|json!({"error":error}))}),
        )
    }
    fn gc(&self, apply: bool, days: u64) -> Result<Value> {
        let bindings = self.bindings()?;
        let pins: HashSet<String> = bindings
            .iter()
            .flat_map(|r| {
                ["current", "pending", "rollback"]
                    .into_iter()
                    .filter_map(move |k| r[k].as_str().map(str::to_owned))
            })
            .collect();
        let mut rows = Vec::new();
        let mut temporary = Vec::new();
        for area in ["staging", "trash"] {
            for item in fs::read_dir(self.root.join(area)).map_err(io_error)? {
                let path = item.map_err(io_error)?.path();
                let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
                if linked(&metadata) {
                    return Err("Managed temporary entry became a link".into());
                }
                let age = now().saturating_sub(
                    metadata
                        .modified()
                        .map_err(io_error)?
                        .duration_since(UNIX_EPOCH)
                        .map_err(io_error)?
                        .as_secs(),
                );
                if age < days.max(7).saturating_mul(86400) {
                    continue;
                }
                let recognized = if metadata.is_file() {
                    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                        n.starts_with("binding-")
                            && n.ends_with(".json")
                            && uuid::Uuid::parse_str(&n[8..n.len() - 5]).is_ok()
                    })
                } else if metadata.is_dir() {
                    let marker = path.join(if area == "staging" {
                        "STAGE.json"
                    } else {
                        "PACKAGE.json"
                    });
                    if marker.exists() {
                        regular(&marker, false)?;
                        let record = json(&marker)?;
                        if area == "staging" {
                            record["kind"] == "dtw-owned-stage"
                                && uuid::Uuid::parse_str(
                                    path.file_name().and_then(|v| v.to_str()).unwrap_or(""),
                                )
                                .is_ok()
                                && record["id"]
                                    .as_str()
                                    .is_some_and(|id| package_id(id).is_ok())
                        } else {
                            record["schema"] == SCHEMA
                                && record["id"].as_str()
                                    == path.file_name().and_then(|v| v.to_str())
                                && record["id"]
                                    .as_str()
                                    .is_some_and(|id| package_id(id).is_ok())
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !recognized {
                    continue;
                }
                let error = if apply {
                    if metadata.is_dir() {
                        remove_owned_tree(&path).err()
                    } else {
                        fs::remove_file(&path).err().map(io_error)
                    }
                } else {
                    None
                };
                temporary.push(json!({"path":text(&path)?,"action":if error.is_some(){"blocked"}else if apply{"removed"}else{"would_remove"},"error":error.map(|e|e.to_string())}));
            }
        }
        for entry in fs::read_dir(self.root.join("packages")).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let id = entry
                .file_name()
                .into_string()
                .map_err(|_| "Invalid managed directory name")?;
            let directory = self.package(&id)?;
            let record = json(&directory.join("PACKAGE.json"))?;
            let age = now().saturating_sub(
                record["imported_at"]
                    .as_u64()
                    .ok_or("Missing import time")?,
            );
            let lease_path = self.root.join("leases").join(format!("{id}.lock"));
            let lease = open_lock_file(&lease_path)?;
            let reason = if pins.contains(&id) {
                Some("host_pin")
            } else if age < days.saturating_mul(86400) {
                Some("retention")
            } else {
                match lease.try_lock() {
                    Ok(()) => None,
                    Err(std::fs::TryLockError::WouldBlock) => Some("running"),
                    Err(e) => return Err(io_error(e)),
                }
            };
            let mut removal_error = None;
            if reason.is_none() && apply {
                let trash = self.root.join("trash").join(&id);
                // A rename failure (including a Windows executable still in use)
                // preserves the package and reports it; never kill a consumer.
                removal_error = fs::rename(&directory, &trash)
                    .map_err(io_error)
                    .and_then(|_| remove_owned_tree(&trash))
                    .err();
            }
            drop(lease);
            if reason.is_none() && apply && removal_error.is_none() {
                let _ = fs::remove_file(&lease_path);
            }
            rows.push(json!({"package_id":id,"retained_by":reason,"action":if removal_error.is_some(){"blocked"}else if reason.is_some(){"keep"}else if apply{"removed"}else{"would_remove"},"error":removal_error}));
        }
        Ok(
            json!({"apply":apply,"packages":rows,"temporary":temporary,"scope":"managed package store only; unmanaged installations and Core bundles are untouched"}),
        )
    }
}

/// Every managed process holds a shared lifetime lease; GC takes it exclusively.
/// Workers protect themselves even if their supervisor exits unexpectedly.
pub fn runtime_lease() -> Result<Option<File>> {
    let executable =
        fs::canonicalize(std::env::current_exe().map_err(io_error)?).map_err(io_error)?;
    let Some(payload) = executable.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    let Some(package) = payload.parent() else {
        return Ok(None);
    };
    if payload.file_name() != Some(std::ffi::OsStr::new("payload"))
        || !package.join("PACKAGE.json").is_file()
    {
        return Ok(None);
    }
    let id = package
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("Invalid managed package path")?;
    package_id(id)?;
    let root = package
        .parent()
        .and_then(Path::parent)
        .ok_or("Invalid managed store path")?;
    let store = Store::open(root)?;
    let expected = store.package(id)?.join("payload").join(if cfg!(windows) {
        "bin/dtw.exe"
    } else {
        "bin/dtw"
    });
    if expected != executable {
        return Err("Managed executable path mismatch".into());
    }
    let lease_path = store.root.join("leases").join(format!("{id}.lock"));
    let lease = open_lock_file(&lease_path)?;
    lease.try_lock_shared().map_err(io_error)?;
    Ok(Some(lease))
}

const USAGE: &str = "Usage: dtw package import <verified-directory> | bind <host-profile> <package-id> | activate <host-profile> | unbind <host-profile> | status | gc [--apply] [--store <absolute-directory>] | --help";

pub fn run(args: &[String]) -> Result<Value> {
    let mut args = args.to_vec();
    let store = if let Some(i) = args.iter().position(|a| a == "--store") {
        if i + 1 >= args.len() {
            return Err("--store requires an absolute path".into());
        }
        let p = PathBuf::from(args.remove(i + 1));
        args.remove(i);
        if !p.is_absolute() || p.parent().is_none() {
            return Err("Store must be an absolute dedicated directory".into());
        }
        Some(p)
    } else {
        None
    };
    let parts: Vec<_> = args.iter().map(String::as_str).collect();
    // Validate syntax and identifiers before resolving or touching the store.
    // Help stays machine-readable, like every successful package command.
    match parts.as_slice() {
        [] | ["--help" | "-h"] => {
            return Ok(
                json!({"usage": USAGE, "manager_version": env!("CARGO_PKG_VERSION"), "store_schema": SCHEMA}),
            );
        }
        ["import", source] if !source.is_empty() && !source.starts_with('-') => {}
        ["bind", host, id] => {
            identifier(host)?;
            package_id(id)?;
        }
        ["activate" | "unbind", host] => identifier(host)?,
        ["status"] | ["gc"] | ["gc", "--apply"] => {}
        _ => return Err(USAGE.into()),
    }
    let store = Store::open(&match store {
        Some(path) => path,
        None => default_store()?,
    })?;
    let result: Result<Value> = match parts.as_slice() {
        ["import", source] => store.import(Path::new(source)),
        ["bind", host, id] => store.bind(host, id),
        ["activate", host] => store.activate(host),
        ["unbind", host] => {
            let p = store.host(host)?;
            if p.exists() {
                regular(&p, false)?;
                fs::remove_file(p).map_err(io_error)?;
            }
            Ok(json!({"host":host,"unbound":true}))
        }
        ["status"] => {
            let mut packages = Vec::new();
            for entry in fs::read_dir(store.root.join("packages")).map_err(io_error)? {
                let id = entry
                    .map_err(io_error)?
                    .file_name()
                    .into_string()
                    .map_err(|_| "Invalid package name")?;
                packages.push(store.recipe(&id)?);
            }
            Ok(json!({"store":text(&store.root)?,"bindings":store.bindings()?,"packages":packages}))
        }
        ["gc"] => store.gc(false, 7),
        ["gc", "--apply"] => store.gc(true, 7),
        _ => unreachable!("package arguments validated before opening the store"),
    };
    result.map(|mut value| {
        value["manager_version"] = json!(env!("CARGO_PKG_VERSION"));
        value["store_schema"] = json!(SCHEMA);
        value
    })
}

#[cfg(test)]
mod tests;

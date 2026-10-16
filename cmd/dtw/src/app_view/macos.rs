//! A short-lived native extension process. The MCP runtime never opens an SCK
//! client: multiple live clients of the same executable can stall image callbacks.
use serde_json::Value;
use std::{
    ffi::{CStr, CString},
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const HELPER: &str = "--dtw-scene-helper";
const MAX_REPLY: u64 = 24 * 1024 * 1024;
unsafe extern "C" {
    fn dtw_scene_call(request: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    fn dtw_scene_free(reply: *mut std::ffi::c_char);
}
fn native(request: Value) -> Result<Value, String> {
    let request = CString::new(request.to_string()).map_err(|e| e.to_string())?;
    unsafe {
        let reply = dtw_scene_call(request.as_ptr());
        if reply.is_null() {
            return Err("macOS extension returned no response".into());
        }
        let result =
            serde_json::from_slice(CStr::from_ptr(reply).to_bytes()).map_err(|e| e.to_string());
        dtw_scene_free(reply);
        result
    }
}
/// Internal pipe protocol, not an MCP tool or an authorization boundary. The
/// child inherits the launcher's OS permissions and never starts a CUA worker.
pub(super) fn helper_entry() -> bool {
    if std::env::args().nth(1).as_deref() != Some(HELPER) {
        return false;
    }
    let result = (|| {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(4097)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 4096 {
            return Err("Native request too large".into());
        }
        native(serde_json::from_slice(&bytes).map_err(|e| e.to_string())?)
    })();
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
    true
}
fn lock_scene() -> Result<File, String> {
    let uid = unsafe { libc::geteuid() };
    // Stable across host-specific TMPDIRs and installed versions. No symlink,
    // foreign-owned file, or group/world-accessible lock is accepted.
    let path = format!("/private/tmp/dev.caelis.dtw-scene-{uid}.lock");
    lock_scene_at(std::path::Path::new(&path), Duration::from_secs(3))
}
fn lock_scene_at(path: &std::path::Path, timeout: Duration) -> Result<File, String> {
    let uid = unsafe { libc::geteuid() };
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err("Native scene lock has an unexpected owner or mode".into());
    }
    let start = Instant::now();
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(file);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::WouldBlock {
            return Err(error.to_string());
        }
        if start.elapsed() >= timeout {
            return Err("native_scene_busy: another Runtime owns the native operation; no new input dispatched".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(super) fn call(request: Value) -> Result<Value, String> {
    // The parent keeps this lock until the child has exited, not merely until a
    // screenshot callback returns. This releases the OS capture client as well.
    let _lease = lock_scene()?;
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg(HELPER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| e.to_string())?;
    let write = child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes());
    if let Err(error) = write {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.to_string());
    }
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_REPLY + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if start.elapsed() < Duration::from_secs(25) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(
                    "Native extension timed out; reconcile any input before continuing".to_owned(),
                );
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error.to_string());
            }
        }
    };
    let output = reader
        .join()
        .map_err(|_| "Native response reader failed".to_owned())?
        .map_err(|e| e.to_string())?;
    if !status?.success() {
        return Err(
            "Native extension exited without a complete result; input outcome may be unknown"
                .into(),
        );
    }
    if output.len() as u64 > MAX_REPLY {
        return Err("Native response exceeded the size limit".into());
    }
    serde_json::from_slice(&output).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    #[test]
    fn lease_rejects_overlap_and_unsafe_files_then_releases_on_drop() {
        let root = std::env::temp_dir().join(format!("dtw-lease-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("lease");
        let first = lock_scene_at(&path, Duration::ZERO).unwrap();
        assert!(lock_scene_at(&path, Duration::ZERO)
            .unwrap_err()
            .contains("native_scene_busy"));
        drop(first);
        drop(lock_scene_at(&path, Duration::ZERO).unwrap());
        let link = root.join("link");
        symlink(&path, &link).unwrap();
        assert!(lock_scene_at(&link, Duration::ZERO).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(lock_scene_at(&path, Duration::ZERO)
            .unwrap_err()
            .contains("owner or mode"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

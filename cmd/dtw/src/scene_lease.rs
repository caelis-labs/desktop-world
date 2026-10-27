//! Shared SCK lease: held through process exit and Swift client teardown.
use std::{
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    time::{Duration, Instant},
};
pub(crate) fn lock_scene() -> Result<File, String> {
    let uid = unsafe { libc::geteuid() };
    // Stable across host-specific TMPDIRs and installed versions. No symlink,
    // foreign-owned file, or group/world-accessible lock is accepted.
    let path = format!("/private/tmp/dev.caelis.dtw-scene-{uid}.lock");
    lock_scene_at(std::path::Path::new(&path), Duration::from_secs(3))
}
pub(crate) fn lock_scene_at(path: &std::path::Path, timeout: Duration) -> Result<File, String> {
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

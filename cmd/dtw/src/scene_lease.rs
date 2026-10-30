//! Cross-process native resource leases. Input lock order: coordination, app instance, physical input.
//! SCK ownership is held through process exit and Swift client teardown.
use std::{
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    time::{Duration, Instant},
};
pub(crate) fn lock_scene(timeout: Duration) -> Result<File, String> {
    let uid = unsafe { libc::geteuid() };
    // Stable across host-specific TMPDIRs and installed versions. No symlink,
    // foreign-owned file, or group/world-accessible lock is accepted.
    let path = format!("/private/tmp/dev.caelis.dtw-scene-{uid}.lock");
    lock_scene_at(std::path::Path::new(&path), timeout)
}
// This separate lease is scoped to one complete atomic input transaction.
// Worker process exit releases it only after the native input drain (or kill).
pub(crate) fn lock_input(timeout: Duration) -> Result<File, String> {
    let uid = unsafe { libc::geteuid() };
    lock_scene_at(
        std::path::Path::new(&format!("/private/tmp/dev.caelis.dtw-input-{uid}.lock")),
        timeout,
    )
}
/// Serialize mutations of one process incarnation across independent MCP
/// sessions, without blocking background work in unrelated applications.
pub(crate) fn lock_instance(pid: u32, generation: u64, timeout: Duration) -> Result<File, String> {
    let uid = unsafe { libc::geteuid() };
    lock_scene_at(
        std::path::Path::new(&format!(
            "/private/tmp/dev.caelis.dtw-app-{uid}-{pid}-{generation}.lock"
        )),
        timeout,
    )
}
/// Directed and semantic mutations can overlap across applications. A foreground
/// transaction excludes them until its inline return finishes, because return
/// can mutate a different application's focused window. Standalone return and
/// shutdown use the same exclusive admission, before any instance/input lock.
#[derive(Clone, Copy)]
pub(crate) enum InputCoordination {
    Background,
    Foreground,
}
pub(crate) fn lock_coordination(
    mode: InputCoordination,
    timeout: Duration,
) -> Result<File, String> {
    let uid = unsafe { libc::geteuid() };
    lock_at(
        std::path::Path::new(&format!(
            "/private/tmp/dev.caelis.dtw-coordination-{uid}.lock"
        )),
        timeout,
        match mode {
            InputCoordination::Background => libc::LOCK_SH,
            InputCoordination::Foreground => libc::LOCK_EX,
        },
    )
}
pub(crate) fn lock_scene_at(path: &std::path::Path, timeout: Duration) -> Result<File, String> {
    lock_at(path, timeout, libc::LOCK_EX)
}
fn lock_at(path: &std::path::Path, timeout: Duration, mode: libc::c_int) -> Result<File, String> {
    let uid = unsafe { libc::geteuid() };
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err("Native resource lock has an unexpected owner or mode".into());
    }
    let start = Instant::now();
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), mode | libc::LOCK_NB) } == 0 {
            return Ok(file);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::WouldBlock {
            return Err(error.to_string());
        }
        if start.elapsed() >= timeout {
            return Err(
                "Native resource remained busy within this call budget; no new input dispatched"
                    .into(),
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    #[test]
    fn global_input_lease_blocks_other_process_until_its_owner_exits() {
        let path =
            std::env::temp_dir().join(format!("dtw-input-lease-{}.lock", uuid::Uuid::new_v4()));
        let first = lock_scene_at(&path, Duration::from_secs(1)).unwrap();
        drop(first);
        let mut holder = Command::new("python3").args(["-u", "-c",
            "import fcntl,sys\nf=open(sys.argv[1],'r+')\nfcntl.flock(f,fcntl.LOCK_EX)\nprint('owned',flush=True)\nsys.stdin.readline()"])
            .arg(&path).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut ready = String::new();
        BufReader::new(holder.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready.trim(), "owned");
        assert!(lock_scene_at(&path, Duration::from_millis(20)).is_err());
        holder.kill().unwrap();
        holder.wait().unwrap();
        let successor = lock_scene_at(&path, Duration::from_secs(1)).unwrap();
        drop(successor);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn instance_leases_allow_independent_apps_but_serialize_one_incarnation() {
        let generation = (uuid::Uuid::new_v4().as_u128() >> 64) as u64;
        let pid = std::process::id();
        let first = lock_instance(pid, generation, Duration::ZERO).unwrap();
        assert!(lock_instance(pid, generation, Duration::ZERO).is_err());
        let peer = lock_instance(pid, generation.wrapping_add(1), Duration::ZERO).unwrap();
        drop(first);
        drop(lock_instance(pid, generation, Duration::ZERO).unwrap());
        drop(peer);
        let uid = unsafe { libc::geteuid() };
        for value in [generation, generation.wrapping_add(1)] {
            std::fs::remove_file(format!(
                "/private/tmp/dev.caelis.dtw-app-{uid}-{pid}-{value}.lock"
            ))
            .unwrap();
        }
    }

    #[test]
    fn coordination_allows_parallel_background_but_blocks_return_until_it_finishes() {
        let path =
            std::env::temp_dir().join(format!("dtw-coordination-{}.lock", uuid::Uuid::new_v4()));
        let first = lock_at(&path, Duration::ZERO, libc::LOCK_SH).unwrap();
        let mut peer = Command::new("python3").args(["-u", "-c",
            "import fcntl,sys\nf=open(sys.argv[1],'r+')\nfcntl.flock(f,fcntl.LOCK_SH|fcntl.LOCK_NB)\nprint('background-running',flush=True)\nsys.stdin.readline()"])
            .arg(&path).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut ready = String::new();
        BufReader::new(peer.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready.trim(), "background-running");
        // Both independent background calls own shared leases. A bounded return
        // times out without stealing either lease or admitting a physical write.
        let started = Instant::now();
        assert!(lock_at(&path, Duration::from_millis(20), libc::LOCK_EX).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(first);
        assert!(lock_at(&path, Duration::ZERO, libc::LOCK_EX).is_err());
        // Cancellation/process exit releases the remaining owner for cleanup.
        peer.kill().unwrap();
        peer.wait().unwrap();
        drop(lock_at(&path, Duration::from_secs(1), libc::LOCK_EX).unwrap());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn foreground_transaction_excludes_background_until_return_and_owner_exit() {
        let path = std::env::temp_dir().join(format!("dtw-return-{}.lock", uuid::Uuid::new_v4()));
        drop(lock_scene_at(&path, Duration::ZERO).unwrap());
        let mut holder = Command::new("python3").args(["-u", "-c",
            "import fcntl,sys\nf=open(sys.argv[1],'r+')\nfcntl.flock(f,fcntl.LOCK_EX)\nprint('borrowed',flush=True)\nsys.stdin.readline()\nprint('returned',flush=True)\nf.close()"])
            .arg(&path).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut output = BufReader::new(holder.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "borrowed");
        assert!(lock_at(&path, Duration::from_millis(20), libc::LOCK_SH).is_err());
        use std::io::Write;
        holder.stdin.take().unwrap().write_all(b"return\n").unwrap();
        line.clear();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "returned");
        assert!(holder.wait().unwrap().success());
        drop(lock_at(&path, Duration::from_secs(1), libc::LOCK_SH).unwrap());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn resource_lock_refuses_symlink_fifo_and_public_permissions() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = std::env::temp_dir().join(format!("dtw-resource-lock-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let regular = dir.join("regular");
        drop(lock_scene_at(&regular, Duration::ZERO).unwrap());
        let link = dir.join("link");
        symlink(&regular, &link).unwrap();
        assert!(lock_scene_at(&link, Duration::ZERO).is_err());
        std::fs::set_permissions(&regular, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(lock_scene_at(&regular, Duration::ZERO).is_err());
        let fifo = dir.join("fifo");
        let name = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(lock_scene_at(&fifo, Duration::ZERO).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

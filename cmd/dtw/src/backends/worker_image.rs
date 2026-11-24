//! A Unix worker must execute the same image as its supervisor. A pathname
//! check alone cannot prove what the kernel actually loaded across a slot swap.
use super::process::{TransportError, Worker};
use std::{fs, io, os::unix::fs::MetadataExt, path::PathBuf, sync::OnceLock};

static LAUNCH_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
const REPLACED: &str = "Current DTW executable was replaced on disk; restart the MCP server to use the updated package";

fn launch_path() -> Option<&'static PathBuf> {
    LAUNCH_PATH
        .get_or_init(|| std::env::current_exe().ok())
        .as_ref()
}

/// Remember the host-visible path, even on Linux where /proc/self/exe will
/// follow the old inode to the backup after a rename.
pub fn record_launch_path() {
    let _ = launch_path();
}

#[cfg(target_os = "linux")]
fn mapped_image() -> io::Result<(u64, u64)> {
    let image = fs::metadata("/proc/self/exe")?;
    Ok((image.dev(), image.ino()))
}

#[cfg(target_os = "macos")]
fn mapped_image() -> io::Result<(u64, u64)> {
    unsafe extern "C" {
        fn dtw_mapped_executable_identity(device: *mut u64, inode: *mut u64) -> bool;
    }
    let (mut dev, mut ino) = (0, 0);
    // The bridge writes both outputs only on successful lookup of our own image.
    if unsafe { dtw_mapped_executable_identity(&mut dev, &mut ino) } {
        Ok((dev, ino))
    } else {
        Err(io::Error::other(
            "Cannot identify the mapped DTW executable",
        ))
    }
}

pub(super) fn spawn(args: &[&str]) -> Result<Worker, TransportError> {
    let path = launch_path()
        .ok_or_else(|| TransportError::spawn(io::Error::other("Cannot resolve DTW launch path")))?;
    let (dev, ino) = mapped_image().map_err(TransportError::spawn)?;
    if !fs::metadata(path).is_ok_and(|m| (m.dev(), m.ino()) == (dev, ino)) {
        return Err(TransportError::spawn(io::Error::other(REPLACED)));
    }
    // Older binaries don't recognize this flag. New binaries compare against
    // their mapped inode *after exec*, before dispatching even one operation.
    let guarded = format!("{}@{dev}:{ino}", args[0]);
    let mut args = args.to_vec();
    args[0] = &guarded;
    Worker::spawn(path, &args)
}

pub(super) fn admitted_role(flag: &str) -> Option<&str> {
    // Older supervisors launch untagged workers, even across a slot swap.
    let (role, image) = flag.split_once('@')?;
    let (dev, ino) = mapped_image().ok()?;
    (image == format!("{dev}:{ino}")).then_some(role)
}

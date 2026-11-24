//! Internal native checkpoint implementations. Not a public transport.
#[cfg(any(target_os = "windows", target_os = "linux", test))]
pub(crate) mod error_wire;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod process;
#[cfg(any(target_os = "windows", target_os = "linux", test))]
mod text_page;

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod input_sequence;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod native_client;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(unix)]
static STARTUP_EXE: std::sync::OnceLock<Option<(u64, u64)>> = std::sync::OnceLock::new();

#[cfg(unix)]
fn exe_identity(path: &std::path::Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.dev(), meta.ino()))
}

#[cfg(unix)]
fn exe_identity_matches(expected: Option<(u64, u64)>, current: &std::path::Path) -> bool {
    match (expected, exe_identity(current)) {
        (Some(expected), Some(actual)) => expected == actual,
        _ => false,
    }
}

/// Records the startup executable identity so helper spawns fail after a swap.
pub fn record_current_exe_identity() {
    #[cfg(unix)]
    let _ = STARTUP_EXE.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|path| exe_identity(&path))
    });
}

/// Resolves `current_exe` and verifies on Unix that it still matches startup.
pub fn current_worker_exe() -> std::io::Result<std::path::PathBuf> {
    let path = std::env::current_exe()?;
    #[cfg(unix)]
    {
        let expected = *STARTUP_EXE.get_or_init(|| exe_identity(&path));
        if !exe_identity_matches(expected, &path) {
            return Err(std::io::Error::other(
                "Current DTW executable was replaced on disk; restart the MCP server to use the updated package",
            ));
        }
    }
    Ok(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn exe_identity_rejects_replaced_or_missing_binary() {
        let dir = std::env::temp_dir().join(format!("dtw-exe-id-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let file = dir.join("dtw");
        fs::write(&file, "v1").unwrap();
        let expected = exe_identity(&file);
        assert!(expected.is_some());
        assert!(exe_identity_matches(expected, &file));

        let replacement = dir.join("dtw.new");
        fs::write(&replacement, "v2").unwrap();
        fs::rename(&replacement, &file).unwrap();
        assert!(!exe_identity_matches(expected, &file));

        fs::remove_file(&file).unwrap();
        assert!(!exe_identity_matches(expected, &file));
        let _ = fs::remove_dir_all(&dir);
    }
}

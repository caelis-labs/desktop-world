//! Per-runtime worker supervision. Cancellation ends this runtime; never replay.
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

pub struct Lifetime {
    pub root: PathBuf,
}
impl Lifetime {
    pub fn new(worker: &str) -> Result<Self, String> {
        let root = env::temp_dir().join(format!("dtw-lifetime-{}", uuid::Uuid::new_v4()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&root)
                .map_err(|e| e.to_string())?;
        }
        #[cfg(not(unix))]
        fs::create_dir(&root).map_err(|e| e.to_string())?;
        fs::write(
            root.join("worker.json"),
            serde_json::json!({"worker":worker,"parent":std::process::id()}).to_string(),
        )
        .map_err(|e| e.to_string())?;
        env::set_var("DTW_STOP_FILE", root.join("stop"));
        Ok(Self { root })
    }
    pub fn stop(&self) -> Result<(), String> {
        fs::write(self.root.join("stop"), b"stop").map_err(|e| e.to_string())
    }
    pub fn stopped(&self) -> bool {
        self.root.join("stop").exists()
    }
}
impl Drop for Lifetime {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(unix)]
pub fn parent_alive(pid: u32) -> bool {
    unsafe { libc::getppid() as u32 == pid }
}
#[cfg(windows)]
pub fn parent_alive(pid: u32) -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn WaitForSingleObject(handle: *mut std::ffi::c_void, millis: u32) -> u32;

    }
    unsafe {
        // Keep the original kernel object, so PID reuse cannot extend ownership.
        static PARENT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let handle = *PARENT.get_or_init(|| OpenProcess(0x00100000, 0, pid) as usize)
            as *mut std::ffi::c_void;
        if handle.is_null() {
            return false;
        }
        WaitForSingleObject(handle, 0) == 258
    }
}

pub fn supervisor_entry() -> bool {
    if env::args().nth(1).as_deref() != Some("__private-worker") {
        return false;
    }
    let result = (|| -> Result<(), String> {
        let root = env::temp_dir();
        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("worker.json")).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let parent = config["parent"].as_u64().ok_or("missing parent")? as u32;
        if !parent_alive(parent) || root.join("stop").exists() {
            return Err("runtime stopped".into());
        }
        let mut child = Command::new(config["worker"].as_str().ok_or("missing worker")?)
            .args(env::args().skip(1))
            // Host-owned native lifecycle configuration, not permission/grant
            // evidence. The SDK's general environment allowlist excludes this
            // native maintenance setting. Preserve the previous delegated
            // session's 30-minute cleanup ceiling at our actual worker launch;
            // DTW refuses new work at 25 minutes of context age. Native manifest
            // and authorization-grant expiry remain independently enforced.
            .env("CUA_DRIVER_RS_SESSION_IDLE_TTL_SECS", "1800")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| e.to_string())?;
        loop {
            if !parent_alive(parent) || root.join("stop").exists() {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        return Ok(());
                    }
                    return Err(format!("worker exited: {status}"));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(e.to_string());
                }
            }
        }
    })();
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
    true
}

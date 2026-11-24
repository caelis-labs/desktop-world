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
mod worker_image;
#[cfg(unix)]
pub use worker_image::record_launch_path;

/// Spawn only this executable's worker, preserving Windows cooperative shutdown.
fn spawn_current_worker(args: &[&str]) -> Result<process::Worker, process::TransportError> {
    #[cfg(unix)]
    return worker_image::spawn(args);
    #[cfg(windows)]
    {
        let path = std::env::current_exe().map_err(process::TransportError::spawn)?;
        process::Worker::spawn_cooperative(&path, args)
    }
}

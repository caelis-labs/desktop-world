//! Internal native checkpoint implementations. Not a public transport.
#[cfg(target_os = "macos")]
pub mod macos;
pub mod process;
#[cfg(any(target_os = "windows", test))]
mod text_page;

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod input_sequence;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod native_client;
#[cfg(target_os = "windows")]
pub mod windows;

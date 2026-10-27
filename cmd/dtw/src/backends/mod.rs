//! Internal native checkpoint implementations. Not a public transport.
#[cfg(target_os = "macos")]
pub mod macos;
pub mod process;

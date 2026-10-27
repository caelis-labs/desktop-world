//! The next DTW execution core. Internal milestone API, not the MCP contract.
//!
//! Kept dependency-free so its invariants can run before native package builds.
//! The default executable path has not switched to this implementation.
//! The app-protocol feature provides a separate opt-in MCP entry.

pub mod computer;
pub mod runtime;

#[cfg(feature = "native-checkpoints")]
pub mod backends;

#[cfg(feature = "app-protocol")]
pub mod app_protocol;

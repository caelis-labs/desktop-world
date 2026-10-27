//! The next DTW execution core. Internal milestone API, not the MCP contract.
//!
//! Kept dependency-free so its invariants can run before native package builds.
//! The existing executable has not switched to this implementation yet.

pub mod computer;
pub mod runtime;

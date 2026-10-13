//! GitHub Actions workflow YAML rendering (Gate 0 shell).
//!
//! Owns generic workflow YAML from typed workflow IR. Must not own
//! Rust/Cargo, Mise syntax, repository scanning, subprocesses, or stack
//! policy.

/// Version marker for the renderer shell.
pub const RENDERER_VERSION: u32 = 0;

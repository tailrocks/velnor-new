//! Mise tool selection and pinned command construction (Gate 0 shell).
//!
//! Owns the subprocess/environment wrapper, `mise.toml`/`mise.lock`
//! inspection, and Mise cache integration. Must not own Cargo metadata,
//! Rust graph rules, `rust-toolchain.toml`, GitHub YAML, or stack discovery.

/// Stable identifier for the Mise tool wrapper.
pub const TOOL_ID: &str = "mise";

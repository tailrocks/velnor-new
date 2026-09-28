//! Rust/Cargo stack discovery and task proposals (Gate 0 shell).
//!
//! Owns metadata conversion, targets, graph, affected selection, and
//! `rust-toolchain.toml` inspection. Must not own Mise, workflow YAML,
//! process execution, or non-Rust stack behavior.

/// Stable identifier for the Rust stack.
pub const STACK_ID: &str = "rust";

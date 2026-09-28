//! Stack-neutral workflow/task contracts (Gate 0 shell).
//!
//! Owns task graphs, identities, reports, and recommendations. Must not own
//! Rust/Cargo, Mise, process, filesystem, YAML, CLI, or generic app models.

/// Version marker for the contract schema shell.
pub const CONTRACT_VERSION: u32 = 0;

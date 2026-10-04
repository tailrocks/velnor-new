//! Stack-neutral workflow/task contracts.
//!
//! Owns task graphs, identities, reports, and recommendations. Must not own
//! Rust/Cargo, Mise, process, filesystem, YAML, CLI, or generic app models.
//!
//! All types here are effect-free data plus pure derivation/validation.
//! Derivation formulas are normative; example strings in docs are illustrative.

pub mod archive;
pub mod branch;
pub mod cachekey;
pub mod candidate_manifest;
pub mod canonical;
pub mod closure;
mod compiled_support;
pub mod config;
pub mod discover;
pub mod errors;
mod exports;
pub mod extension_schemas;
pub mod extensions;
pub mod finding;
pub mod formats;
pub mod freshness;
pub mod git_ref;
pub mod graph;
pub mod ids;
pub mod manifest;
pub(crate) mod manifest_checks;
pub mod marker;
pub mod policy;
pub mod propose;
pub mod secrets;
pub mod strict_json;
pub mod targets;
mod task_digest;
pub use task_digest::canonical_task_digest;
mod tool_targets;
pub mod tooling;
pub mod vcs;
pub mod workflow;
pub use exports::*;

/// Version marker for the contract schema shell.
pub const CONTRACT_VERSION: u32 = 0;

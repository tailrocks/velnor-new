//! Rust/Cargo stack discovery and task proposals.
//!
//! Pure inventory, evidence, and task-group derivation from bytes the
//! orchestrator supplies. This crate launches no processes, builds no tool
//! invocations, reads no files itself, and renders no workflow text. It owns
//! read-only inspection of supplied `rust-toolchain.toml` bytes.

mod argv;
pub mod cargo_env;
pub mod closure;
mod closure_probes;
pub mod detect;
pub mod identity;
pub mod propose;
pub mod stability;
mod task_identity;
pub mod tasks;

pub use cargo_env::{DENY_WARNINGS, RUSTDOCFLAGS_ENV, cargo_payload_env};
pub use closure::{lock_digest_at_root, nextest_digest_at_root, resolve_closure_at_root};
pub use detect::{
    CargoCandidate, detected_projects_for_units, discover_candidates, discover_stack_candidates,
    manifest_for_key, manifest_for_unit_root, project_root_for_manifest,
};
pub use identity::{
    GroupExtensionInputs, adapter_entry_metadata, entry_metadata_for_task, expand_shards_for_group,
    extension_for_proposal,
};
pub use propose::{
    KIND_DISPLAY_WORDS, ToolNeeds, is_clippy_kind, is_nextest_kind, is_workspace_fmt_task,
    payload_env_for_kind, propose_task, resource_class_for_kind, step_base_name, task_kind_rank,
    tool_needs,
};
pub use stability::{
    CommittedProfile, committed_profile_differs, read_committed_profile_for_comparison,
};
pub use tasks::{
    DeriveInputs, TaskGroup, TaskKind, derive_task_groups, derive_workspace_fmt,
    derive_workspace_fmt_if_explicit,
};

/// Stable identifier for the Rust stack.
pub const STACK_ID: &str = "rust";

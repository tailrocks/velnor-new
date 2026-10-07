//! Orchestration substrate: errors, config, scheduling, constrained IO, git plumbing.
//!
//! Bottom leaf of the orchestrator family: every sibling builds on these
//! vocabulary types and pure helpers, and this crate depends on no
//! orchestrator sibling. [`OrchestratorError`] plus the [`internal`] and
//! [`internal_contract`] constructors are the shared failure vocabulary;
//! [`config`] loads `.velnor/config.toml`; [`schedule`] and
//! [`clippy_groups`] hold planner policy; [`safe_read`] and
//! [`exclusive_write`] hold constrained file IO; [`validators`],
//! [`git_paths`], [`origin`], and [`root`] hold git plumbing;
//! [`decisions`] holds planner decisions; [`extension_schemas`],
//! [`init`], [`obligation_order`], [`qualify`], and [`utf8`] hold small
//! shared vocabulary.

pub mod clippy_groups;
pub mod config;
mod config_stacks;
pub mod decisions;
pub mod error;
pub mod exclusive_write;
pub mod extension_schemas;
pub mod git_paths;
pub mod init;
pub mod link_safety;
pub mod obligation_order;
pub mod origin;
pub mod qualify;
pub mod report_keys;
pub mod root;
pub mod safe_read;
pub mod schedule;
pub mod sha256;
pub mod staged_reads;
pub mod utf8;
pub mod validators;

pub use clippy_groups::{ClippyMemoryPlan, clippy_memory_groups};
pub use error::{OrchestratorError, internal, internal_contract};
pub use extension_schemas::{
    coverage_schema_known, extension_schema_for_stack, reuse_eligible_for_schema, task_key_segment,
    task_kind_segment, task_stack_segment,
};
pub use init::{InitReport, init_config};
pub use qualify::qualify_argv_staged;
pub use root::resolve_root;
pub use validators::{
    GitArgError, validate_diff_args, validate_diff_rev, validate_git_args, validate_rev,
    validate_select_diff_args, validate_select_show_args, validate_show_args, validate_show_path,
};

//! Generation coordination: root, config, discovery, IR, and writes.
//!
//! Composes the contract, Rust, Mise, actionlint, and workflow-renderer
//! adapters. This crate launches no child invocations, builds no fixed
//! vectors itself, and assembles no workflow text: execution belongs to
//! Mise, vectors to `vectors` via Mise requests, text to the renderer.

mod api;
mod cover_baseline;
pub use velnor_actions_orchestrator_workflow_ir::crate_jobs;
mod internal;
mod internal_request;
mod merge;
mod merge_request;
mod retrieve_reports;
pub use velnor_actions_orchestrator_run_select as run_select;
mod task_report;

pub use api::*;

/// Version marker for the orchestrator shell.
pub const ORCHESTRATOR_VERSION: u32 = 0;

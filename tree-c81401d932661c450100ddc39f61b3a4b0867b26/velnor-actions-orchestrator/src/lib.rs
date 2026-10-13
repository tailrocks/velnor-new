//! Generation coordination: root, config, discovery, IR, and writes.
//!
//! Composes the contract, Rust, Mise, actionlint, and workflow-renderer
//! adapters. This crate launches no child invocations, builds no fixed
//! vectors itself, and assembles no workflow text: execution belongs to
//! Mise, vectors to `vectors` via Mise requests, text to the renderer.

mod api;
mod baseline_publish;
mod check_runtime;
mod cover;
mod cover_baseline;
mod cover_compat;
mod cover_identity;
pub use velnor_actions_orchestrator_workflow_ir::crate_jobs;
mod external_data;
mod internal;
mod internal_request;
mod merge;
mod merge_request;
mod preseed_manifest;
mod request_event;
mod retrieve_baseline;
mod retrieve_reports;
mod retrieve_retry;
pub mod run_select;
mod task_report;

pub use api::*;

/// Version marker for the orchestrator shell.
pub const ORCHESTRATOR_VERSION: u32 = 0;

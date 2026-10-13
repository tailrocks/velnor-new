//! Generation coordination: root, config, discovery, IR, and writes.
//!
//! Composes the contract, Rust, Mise, actionlint, and workflow-renderer
//! adapters. This crate launches no child invocations, builds no fixed
//! vectors itself, and assembles no workflow text: execution belongs to
//! Mise, vectors to `vectors` via Mise requests, text to the renderer.

mod api;
mod attach;
mod baseline_publish;
mod check_evidence;
mod check_runtime;
mod cover;
mod cover_baseline;
mod cover_compat;
mod cover_identity;
mod covered_tasks;
mod crate_job_ids;
pub mod crate_jobs;
mod critical_path;
mod external_data;
mod finalized;
mod freshness_emit;
mod generate;
mod internal;
mod internal_request;
mod mbx_preflight;
mod merge;
mod merge_request;
mod noop_report;
mod plan;
mod plan_output_limits;
mod plan_stacks;
mod prepare;
mod preseed_manifest;
mod provenance;
mod publish_job;
mod release_checkouts;
mod release_emit;
mod release_identity;
mod release_steps;
mod request_event;
mod retrieve_baseline;
mod retrieve_reports;
mod retrieve_retry;
mod routing;
pub mod run_select;
mod task_report;
mod task_report_aggregate;
mod validate;
mod validate_shell;
mod validate_zizmor;
mod verification_tasks;
mod workflow;
mod workflow_jobs;
mod workflow_jobs_cache;

pub use api::*;

/// Version marker for the orchestrator shell.
pub const ORCHESTRATOR_VERSION: u32 = 0;

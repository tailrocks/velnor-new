//! Generation coordination: root, config, discovery, IR, and writes.
//!
//! Composes the contract, Rust, Mise, actionlint, and workflow-renderer
//! adapters. This crate launches no child invocations, builds no fixed
//! vectors itself, and assembles no workflow text: execution belongs to
//! Mise, vectors to [`vectors`] via Mise requests, text to the renderer.

mod config;
mod cover;
mod cover_baseline;
mod cover_identity;
mod discover;
mod discover_index;
mod error;
mod evidence;
mod generate;
mod init;
mod internal;
mod internal_plan;
mod internal_request;
mod merge;
mod plan;
mod prepare;
mod root;
mod select;
mod select_affected;
mod select_edges;
mod validate;
mod validate_shell;
mod validate_zizmor;
mod validators;
mod vectors;
mod workflow;
mod workflow_jobs;

pub use discover::{Discovery, PlannedWorkspace};
pub use error::OrchestratorError;
pub use generate::{GenerateOptions, GenerateReport, ToolSnapshot, generate, render_staged_tree};
pub use init::{InitReport, init_config};
pub use internal::{
    MERGE_OP, PLAN_OP, PlanOutputs, REQUEST_FILE_ENV, WRITE_REQUEST_OP, merge_passed,
    plan_internal, plan_outputs, response_path_for, write_request, write_request_parts,
};
pub use merge::merge_internal;
pub use plan::{plan_text, plan_text_checked};
pub use prepare::{GenerationPreparation, prepare};
pub use root::resolve_root;
pub use validators::{
    GitArgError, validate_diff_args, validate_diff_rev, validate_git_args, validate_rev,
    validate_select_diff_args, validate_select_show_args, validate_show_args, validate_show_path,
};
pub use workflow::{CHECKOUT_USES, DEFAULT_RUNNER_LABEL, WorkflowPlan};

/// Version marker for the orchestrator shell.
pub const ORCHESTRATOR_VERSION: u32 = 0;

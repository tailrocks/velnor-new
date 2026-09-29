//! Generation coordination: root, config, discovery, IR, and writes.
//!
//! Composes the contract, Rust, Mise, actionlint, and workflow-renderer
//! adapters. This crate launches no child invocations, builds no fixed
//! vectors itself, and assembles no workflow text: execution belongs to
//! Mise, vectors to [`vectors`] via Mise requests, text to the renderer.

mod attach;
mod clippy_groups;
mod config;
mod cover;
mod cover_baseline;
mod cover_identity;
mod critical_path;
pub mod decisions;
mod discover;
mod discover_index;
mod error;
mod evidence;
mod extension_schemas;
mod external_data;
mod generate;
mod init;
mod internal;
mod internal_plan;
mod internal_request;
mod inventory;
mod merge;
mod merge_request;
mod pins;
mod plan;
mod prepare;
mod qualify;
mod recommendations;
mod retrieve_reports;
mod root;
pub mod schedule;
mod select;
mod select_affected;
mod select_edges;
mod source_prep;
mod toolcheck;
mod toolfindings;
mod utf8;
mod validate;
mod validate_shell;
mod validate_zizmor;
mod validators;
mod vectors;
mod workflow;
mod workflow_jobs;

pub use clippy_groups::{ClippyMemoryPlan, clippy_memory_groups};
pub use critical_path::{
    CriticalPath, critical_path, critical_path_for_groups, critical_path_line,
    critical_path_structural, render_critical_path,
};
pub use discover::{Discovery, PlannedWorkspace};
pub use error::OrchestratorError;
pub use extension_schemas::{
    coverage_schema_known, extension_schema_for_stack, reuse_eligible_for_schema,
    task_kind_segment, task_stack_segment,
};
pub use external_data::{
    DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, EXTERNAL_DATA_CHECK_KIND, ExternalDataFreshness,
    external_data_kind, may_skip_external_data,
};
pub use generate::{GenerateOptions, GenerateReport, ToolSnapshot, generate, render_staged_tree};
pub use init::{InitReport, init_config};
pub use internal::{
    MERGE_OP, PLAN_OP, PlanOutputs, REQUEST_FILE_ENV, WRITE_REQUEST_OP, merge_passed,
    plan_internal, plan_outputs, publish_final_report, publish_plan_files, response_path_for,
    write_request, write_request_parts,
};
pub use merge::merge_internal;
pub use merge_request::assemble_merge_request;
pub use pins::consumer_acquire_step_with_manifest;
pub use plan::{plan_text, plan_text_checked};
pub use prepare::{GenerationPreparation, prepare};
pub use qualify::qualify_argv_staged;
pub use retrieve_reports::{FETCH_OP, retrieve_reports};
pub use root::resolve_root;
pub use toolcheck::{TOOL_INPUT_PATHS, ToolInputCheck, ToolParse, check_tool_inputs};
pub use toolfindings::{
    CONFLICTING_TOOL_VALUES, UNSUPPORTED_TOOL_VALUE, finding_line, tool_check_lines, tool_conflicts,
};
pub use validators::{
    GitArgError, validate_diff_args, validate_diff_rev, validate_git_args, validate_rev,
    validate_select_diff_args, validate_select_show_args, validate_show_args, validate_show_path,
};
pub use workflow::{CHECKOUT_USES, DEFAULT_RUNNER_LABEL, WorkflowPlan};

/// Version marker for the orchestrator shell.
pub const ORCHESTRATOR_VERSION: u32 = 0;

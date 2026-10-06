//! Generation coordination: root, config, discovery, IR, and writes.
//!
//! Composes the contract, Rust, Mise, actionlint, and workflow-renderer
//! adapters. This crate launches no child invocations, builds no fixed
//! vectors itself, and assembles no workflow text: execution belongs to
//! Mise, vectors to `vectors` via Mise requests, text to the renderer.

mod attach;
mod baseline_publish;
mod clippy_groups;
mod config;
mod config_stacks;
mod cover;
mod cover_baseline;
mod cover_compat;
mod cover_identity;
mod covered_tasks;
mod crate_job_ids;
mod crate_jobs;
mod critical_path;
pub mod decisions;
mod derive_groups;
mod discover;
mod discover_index;
mod discover_tofu;
mod error;
mod evidence;
mod exclusive_write;
mod extension_schemas;
mod external_data;
mod finalized;
mod freshness_emit;
mod generate;
mod git_paths;
mod init;
mod internal;
mod internal_plan;
mod internal_request;
mod inventory;
mod inventory_reuse;
mod lock_audit;
mod matrix_step;
mod merge;
mod merge_request;
mod noop_report;
mod origin;
mod pins;
mod plan;
mod plan_stacks;
mod prepare;
mod preseed_manifest;
mod provenance;
mod publish_job;
mod qualify;
mod recommendations;
mod release_checkouts;
mod release_emit;
mod release_identity;
mod release_steps;
mod request_event;
mod retrieve_baseline;
mod retrieve_reports;
mod retrieve_retry;
mod root;
mod routing;
pub mod run_select;
mod safe_read;
pub mod schedule;
mod select;
mod select_affected;
mod select_edges;
mod select_tofu;
mod source_cache;
mod source_prep;
mod task_report;
mod task_report_aggregate;
mod tofu_cache;
mod toolcheck;
mod toolfindings;
mod utf8;
mod validate;
mod validate_shell;
mod validate_zizmor;
mod validators;
mod vectors;
mod verify;
mod workflow;
mod workflow_jobs;
mod workflow_jobs_cache;

pub use baseline_publish::{PUBLISH_OP, PublishOutputs, baseline_publish};
pub use clippy_groups::{ClippyMemoryPlan, clippy_memory_groups};
pub use cover_compat::baseline_artifact_numeric_id;
pub use covered_tasks::COVERED_TASKS_OUTPUT;
pub use critical_path::{
    CriticalPath, critical_path, critical_path_for_groups, critical_path_line,
    critical_path_structural, render_critical_path,
};
pub use derive_groups::FeatureFallback;
pub use discover::{Discovery, PlannedWorkspace};
pub use error::OrchestratorError;
pub use extension_schemas::{
    coverage_schema_known, extension_schema_for_stack, reuse_eligible_for_schema, task_key_segment,
    task_kind_segment, task_stack_segment,
};
pub use external_data::{
    DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, EXTERNAL_DATA_CHECK_KIND, ExternalDataFreshness,
    external_data_kind, may_skip_external_data,
};
pub use finalized::finalized_jobs;
pub use generate::{
    GenerateOptions, GenerateReport, ToolSnapshot, generate, generate_dispatched,
    render_staged_tree, render_staged_tree_with,
};
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
pub use preseed_manifest::{PRESEED_MANIFEST_OP, write_preseed_manifest};
pub use provenance::{EvidenceProvenance, ProfileProvenance};
pub use qualify::qualify_argv_staged;
pub use retrieve_reports::{FETCH_OP, retrieve_reports};
pub use root::resolve_root;
pub use routing::{migrate_config, parse_dispatch_mode};
pub use task_report::{REPORT_OP, write_task_report};
pub use toolcheck::{TOOL_INPUT_PATHS, ToolInputCheck, ToolParse, check_tool_inputs};
pub use toolfindings::{
    CONFLICTING_TOOL_VALUES, UNSUPPORTED_TOOL_VALUE, finding_line, tool_check_lines, tool_conflicts,
};
pub use validators::{
    GitArgError, validate_diff_args, validate_diff_rev, validate_git_args, validate_rev,
    validate_select_diff_args, validate_select_show_args, validate_show_args, validate_show_path,
};
pub use velnor_actions_contract::ExecutionMode;
pub use workflow::{CHECKOUT_USES, DEFAULT_RUNNER_LABEL, WorkflowPlan};

/// Version marker for the orchestrator shell.
pub const ORCHESTRATOR_VERSION: u32 = 0;

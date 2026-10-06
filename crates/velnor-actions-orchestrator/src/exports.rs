//! Public orchestration entrypoints and their bounded data contracts.

pub use crate::action_report::{
    ACTION_BEGIN_OP, ACTION_REPORT_OP, begin_action_report, write_action_report,
};
pub use crate::baseline_publish::{PUBLISH_OP, PublishOutputs, baseline_publish};
pub use crate::clippy_groups::{ClippyMemoryPlan, clippy_memory_groups};
pub use crate::cover_compat::baseline_artifact_numeric_id;
pub use crate::covered_tasks::{COVERED_TASKS_OUTPUT, PLAN_CARGO_FALLBACK_OUTPUT};
pub use crate::critical_path::{
    CriticalPath, critical_path, critical_path_for_groups, critical_path_line,
    critical_path_structural, render_critical_path,
};
pub use crate::derive_groups::FeatureFallback;
pub use crate::discover::{Discovery, PlannedWorkspace};
pub use crate::error::OrchestratorError;
pub use crate::extension_schemas::{
    coverage_schema_known, extension_schema_for_stack, reuse_eligible_for_schema, task_key_segment,
    task_kind_segment, task_stack_segment,
};
pub use crate::external_data::{
    DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, EXTERNAL_DATA_CHECK_KIND, ExternalDataFreshness,
    external_data_kind, may_skip_external_data,
};
pub use crate::finalized::finalized_jobs;
pub use crate::generate::{
    GenerateOptions, GenerateReport, ToolSnapshot, generate, render_staged_tree,
};
pub use crate::helper_obligation_report::{
    HELPER_BEGIN_OP, HELPER_REPORT_OP, begin_helper_obligation_report,
    write_helper_obligation_report,
};
pub use crate::init::{InitReport, init_config};
pub use crate::internal::{
    AnalysisPublicationContext, AnalysisPublicationOutputs, EarlyPlanResult, MERGE_OP, PLAN_OP,
    PlanOutputs, REQUEST_FILE_ENV, WRITE_REQUEST_OP, merge_passed, plan_early_internal,
    plan_internal, plan_internal_with_analysis, plan_outputs, publish_final_report,
    publish_plan_files, read_early_response, response_path_for, validate_early_response,
    write_request, write_request_parts,
};
pub use crate::merge::merge_internal;
pub use crate::merge_request::assemble_merge_request;
pub use crate::owned_tool_preview::{SourceQualificationTrigger, preview_owned_tool_candidates};
pub use crate::pins::consumer_acquire_step_with_manifest;
pub use crate::plan::plan_text_checked;
pub use crate::prepare::{GenerationPreparation, prepare};
pub use crate::preseed_manifest::{PRESEED_MANIFEST_OP, write_preseed_manifest};
pub use crate::provenance::{EvidenceProvenance, ProfileProvenance};
pub use crate::qualify::qualify_argv_staged;
pub use crate::retrieve_reports::{FETCH_OP, retrieve_reports};
pub use crate::root::resolve_root;
pub use crate::rust_report_preexec::{RUST_REPORT_PREEXEC_OP, validate_rust_report_preexec};
pub use crate::task_clock::{START_OP, write_task_start};
pub use crate::task_report::{REPORT_OP, STAGE_REPORTS_OP, stage_reports, write_task_report};
pub use crate::toolcheck::{TOOL_INPUT_PATHS, ToolInputCheck, ToolParse, check_tool_inputs};
pub use crate::toolfindings::{
    CONFLICTING_TOOL_VALUES, UNSUPPORTED_TOOL_VALUE, finding_line, tool_check_lines, tool_conflicts,
};
pub use crate::validators::{
    GitArgError, validate_diff_args, validate_diff_rev, validate_git_args, validate_rev,
    validate_select_diff_args, validate_select_show_args, validate_show_args, validate_show_path,
};
pub use crate::workflow::{CHECKOUT_USES, DEFAULT_RUNNER_LABEL, WorkflowPlan};

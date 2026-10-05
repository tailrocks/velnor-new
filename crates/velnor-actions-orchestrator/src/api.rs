//! Public orchestrator operations and stable adapter values.
pub use crate::baseline_publish::{PUBLISH_OP, PublishOutputs, baseline_publish};
pub use crate::clippy_groups::{ClippyMemoryPlan, clippy_memory_groups};
pub use crate::cover_compat::baseline_artifact_numeric_id;
pub use crate::covered_tasks::COVERED_TASKS_OUTPUT;
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
    GenerateOptions, GenerateReport, ToolSnapshot, generate, generate_dispatched,
    render_staged_tree, render_staged_tree_with,
};
pub use crate::init::{InitReport, init_config};
pub use crate::internal::{
    MERGE_OP, PLAN_OP, PlanOutputs, REQUEST_FILE_ENV, RESOLVE_QUALIFICATION_ADMISSION_OP,
    WRITE_REQUEST_OP, merge_passed, plan_internal, plan_outputs, plan_outputs_with_admission,
    publish_final_report, publish_plan_files, response_path_for, write_request,
    write_request_parts,
};
pub use crate::merge::merge_internal;
pub use crate::merge_request::assemble_merge_request;
pub use crate::pins::consumer_acquire_step_with_manifest;
pub use crate::plan::{plan_text, plan_text_checked};
pub use crate::plan_output_limits::{JOB_OUTPUTS_BUDGET_UTF16_BYTES, PlanOutputMode};
pub use crate::prepare::{GenerationPreparation, prepare};
pub use crate::preseed_manifest::{PRESEED_MANIFEST_OP, write_preseed_manifest};
pub use crate::provenance::{EvidenceProvenance, ProfileProvenance};
pub use crate::qualification_admission::{
    QUALIFICATION_ADMISSION_FILENAME, load_qualification_admission, resolve_qualification_admission,
};
pub use crate::qualify::qualify_argv_staged;
pub use crate::retrieve_reports::{FETCH_OP, retrieve_reports};
pub use crate::root::resolve_root;
pub use crate::routing::{migrate_config, parse_dispatch_mode};
pub use crate::task_report::{REPORT_OP, write_task_report};
pub use crate::toolcheck::{TOOL_INPUT_PATHS, ToolInputCheck, ToolParse, check_tool_inputs};
pub use crate::toolfindings::{
    CONFLICTING_TOOL_VALUES, UNSUPPORTED_TOOL_VALUE, finding_line, tool_check_lines, tool_conflicts,
};
pub use crate::validators::{
    GitArgError, validate_diff_args, validate_diff_rev, validate_git_args, validate_rev,
    validate_select_diff_args, validate_select_show_args, validate_show_args, validate_show_path,
};
pub use crate::workflow::{CHECKOUT_USES, DEFAULT_RUNNER_LABEL, WorkflowPlan};
pub use velnor_actions_contract::ExecutionMode;
pub use velnor_actions_contract::QualificationCacheAdmission;
pub use velnor_actions_contract::workflow::{
    QUALIFICATION_CACHE_ENABLED_OUTPUT, QUALIFICATION_CACHE_WRITE_OUTPUT,
    QUALIFICATION_CAMPAIGN_OUTPUT, QUALIFICATION_PHASE_OUTPUT,
};
pub use velnor_actions_contract::{DYNAMIC_MATRIX_OUTPUT_MODE, PLAN_MATRIX_OUTPUT_MODE_ENV};

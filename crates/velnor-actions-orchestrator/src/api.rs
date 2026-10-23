//! Public orchestrator API re-exports.

#[path = "github_evidence.rs"]
mod github_evidence;
#[path = "github_evidence_http.rs"]
pub(crate) mod github_evidence_http;

pub use super::bun_assets_handoff::{
    ASSET_ID_ENV as BUN_ASSETS_ID_ENV, ASSET_PINS_ENV as BUN_ASSETS_PINS_ENV, BUN_ASSETS_EXPORT_OP,
    BUN_ASSETS_IMPORT_OP, BUN_ASSETS_PREPARE_OP, run_bun_assets_export, run_bun_assets_import,
    run_bun_assets_prepare,
};
pub use super::check_runtime::{EXECUTE_CHECK_OP, execute_check};
pub use super::clippy_groups::{ClippyMemoryPlan, clippy_memory_groups};
pub use super::config::bun_registry_install::{BUN_REGISTRY_INSTALL_OP, run_bun_registry_install};
pub use super::config::deps_lock::{lock_private_sources, read_deps_lock, render_lock};
pub use super::config::gradle_run::{GRADLE_RUN_OP, run_gradle_run};
pub use super::config::playwright_install::{PLAYWRIGHT_INSTALL_OP, run_playwright_install};
pub use super::config::playwright_lock::{read_playwright_lock, render_playwright_lock};
pub use super::config::playwright_resolve::lock_playwright_browsers;
pub use super::config::private_fetch::{PRIVATE_FETCH_OP, PRIVATE_REVOKE_OP, run_private_fetch};
pub use super::config::private_revoke::run_private_revoke;
pub use super::critical_path::{
    CriticalPath, critical_path, critical_path_for_groups, critical_path_line,
    critical_path_structural, render_critical_path,
};
pub use super::derive_groups::FeatureFallback;
pub use super::discover::{Discovery, PlannedWorkspace};
pub use super::error::OrchestratorError;
pub use super::extension_schemas::{
    extension_schema_for_stack, reuse_eligible_for_schema, task_key_segment, task_kind_segment,
    task_stack_segment,
};
pub use super::finalized::finalized_jobs;
pub use super::generate::unmanaged::{UnmanagedReport, unmanaged_report};
pub use super::generate::{
    GenerateOptions, GenerateReport, ToolSnapshot, generate, generate_dispatched,
    render_staged_tree, render_staged_tree_with,
};
pub use super::init::{InitReport, init_config};
#[doc(hidden)]
pub use super::internal::phase_timing::{PlanPhaseTimings, plan_internal_with_phase_timings};
pub use super::internal::validate_plan_response;
pub use super::internal::{
    MERGE_OP, PLAN_OP, PlanOutputs, REQUEST_FILE_ENV, WRITE_REQUEST_OP, merge_passed,
    plan_internal, plan_outputs, plan_outputs_from_staged_admission, plan_outputs_with_admission,
    publish_final_report, publish_plan_files, response_path_for, write_request,
    write_request_parts,
};
pub use super::merge::merge_internal;
pub use super::merge_request::assemble_merge_request;
pub use super::owned_tool_preview::{SourceQualificationTrigger, preview_owned_tool_candidates};
pub use super::pins::{acquire_script_argv, consumer_acquire_step_with_manifest};
pub use super::plan::{plan_text, plan_text_checked};
pub use super::plan_output_limits::{JOB_OUTPUTS_BUDGET_UTF16_BYTES, PlanOutputMode};
pub use super::prepare::{GenerationPreparation, prepare};
pub use super::preseed_manifest::{PRESEED_MANIFEST_OP, write_preseed_manifest};
pub use super::preview_producer::{
    ACQUIRE_RELEASE_LEAVES_OP, ADMIT_RELEASE_AGGREGATE_OP, EXECUTE_RELEASE_PRODUCER_OP,
    acquire_release_leaves, admit_release_aggregate, execute_release_producer,
};
pub use super::provenance::{EvidenceProvenance, ProfileProvenance};
pub use super::qualification_resolver::{
    QUALIFICATION_ADMISSION_FILENAME, QUALIFICATION_RESOLVER_OP, read_qualification_admission,
    resolve_qualification_admission,
};
pub use super::qualify::qualify_argv_staged;
pub use super::retrieve_reports::{FETCH_OP, retrieve_reports};
pub use super::root::resolve_root;
pub use super::routing::{migrate_config, parse_dispatch_mode};
pub use super::tap_merge::review_completion::{
    CAPTURE_REVIEW_COMPLETION_OP, EMIT_REVIEW_COMPLETION_OP, capture_review_completion,
    emit_review_completion,
};
pub use super::tap_merge::{
    MERGE_HOMEBREW_TAP_PR_OP, TapMergeOutcome, merge_homebrew_tap_from_env,
};
pub use super::tap_prepare::{
    ADMIT_HOMEBREW_TAP_OP, HomebrewTapPreparationProof, PREPARE_HOMEBREW_TAP_OP,
    TapPreparationDisposition, TapPromotionOutcome, TapPromotionReceipt, WRITE_HOMEBREW_TAP_PR_OP,
    admit_homebrew_tap_from_env, prepare_homebrew_tap_from_env, write_homebrew_tap_pr_from_env,
};
pub use super::task_execution::{
    GENERATOR_VERSION_ENV, RUNTIME_RUNNER_TEMP_ENV, TASK_EXECUTION_DIGEST_ENV,
    TASK_EXECUTION_RESOLVER_OP, resolve_task_execution,
};
pub use super::task_report::{REPORT_OP, write_task_report};
pub use super::toolcheck::{TOOL_INPUT_PATHS, ToolInputCheck, ToolParse, check_tool_inputs};
pub use super::toolfindings::{
    CONFLICTING_TOOL_VALUES, UNSUPPORTED_TOOL_VALUE, finding_line, tool_check_lines, tool_conflicts,
};
pub use super::validators::{
    GitArgError, validate_diff_args, validate_diff_rev, validate_git_args, validate_rev,
    validate_select_diff_args, validate_select_show_args, validate_show_args, validate_show_path,
};
pub use super::workflow::{CHECKOUT_USES, DEFAULT_RUNNER_LABEL, WorkflowPlan};
pub use github_evidence::{
    GITHUB_EVIDENCE_FETCH_OP, GITHUB_EVIDENCE_IMPORT_OP, run_github_evidence_fetch,
    run_github_evidence_import,
};
pub use velnor_actions_contract::ExecutionMode;
pub use velnor_actions_contract::workflow::{
    QUALIFICATION_CACHE_ENABLED_OUTPUT, QUALIFICATION_CACHE_WRITE_OUTPUT,
    QUALIFICATION_CAMPAIGN_OUTPUT, QUALIFICATION_PHASE_OUTPUT,
};
pub use velnor_actions_contract::{DYNAMIC_MATRIX_OUTPUT_MODE, PLAN_MATRIX_OUTPUT_MODE_ENV};

pub use super::package_publication::{
    PackagePublicationBackend, PackagePublicationOutcome, publish_package_release,
};
pub use super::package_release_publish::release_policy_admission::{
    ACQUIRE_PREVIEW_SOURCE_OP, FINALIZE_PREVIEW_SOURCE_OP, SELECT_PREVIEW_SOURCE_OP,
    acquire_preview_source_from_env, finalize_preview_source_from_env,
    select_preview_source_from_env,
};
pub use super::package_release_publish::{
    ADMIT_STABLE_TAG_ORIGIN_OP, PUBLISH_PACKAGE_RELEASE_OP, RECOVER_PACKAGE_RELEASE_OP,
    ReleaseTapContinuationSelection, SELECT_RELEASE_TAP_CONTINUATION_OP,
    SELECT_STABLE_RELEASE_SOURCE_OP, WRITE_STABLE_TAG_INTENT_OP, WRITE_STABLE_TAG_ORIGIN_OP,
    admit_stable_tag_origin_from_env, publish_package_release_from_env,
    recover_package_release_from_env, select_release_tap_continuation_from_env,
    select_stable_release_source_from_env, write_stable_tag_intent_from_env,
    write_stable_tag_origin_from_env,
};
pub use super::package_release_signer::{SIGN_PACKAGE_RELEASE_OP, sign_package_release_from_env};

/// Version marker for the orchestrator shell.
pub const ORCHESTRATOR_VERSION: u32 = 0;

pub use crate::qualification_candidate::qualify_tools;

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub fn seed_parallel_targets_from_env() -> Result<(), super::OrchestratorError> {
    super::parallel_target_seed::seed_parallel_targets_from_env().map(|_| ())
}

pub use super::homebrew::authoring::{
    HomebrewAdoptionAuthoringReport, author_homebrew_adoption_cutover,
    author_homebrew_adoption_seed,
};
pub use super::homebrew::source_policy::{
    check_homebrew_source_policy, export_homebrew_source_policy,
};

#[path = "bun_assets_repository.rs"]
mod bun_assets_repository;
#[path = "github_evidence.rs"]
mod github_evidence;
#[path = "github_evidence_http.rs"]
pub(crate) mod github_evidence_http;
pub use bun_assets_repository::{
    BUN_ASSETS_REPOSITORY_CHECK_OP, BunAssetsRepositoryRead, check_bun_assets_repository,
    run_bun_assets_repository_check,
};

pub use super::bun_assets_handoff::{
    ASSET_ID_ENV as BUN_ASSETS_ID_ENV, ASSET_PINS_ENV as BUN_ASSETS_PINS_ENV, BUN_ASSETS_EXPORT_OP,
    BUN_ASSETS_IMPORT_OP, BUN_ASSETS_PREPARE_OP, run_bun_assets_export, run_bun_assets_import,
    run_bun_assets_prepare,
};
pub use super::cache_admission::{
    ADMISSION_SCHEMA, AdmissionDecision, AdmissionRequest, AdmissionV1,
    BLOCKED_GENERATION_SLOTS_EXHAUSTED, BLOCKED_INSUFFICIENT_HEADROOM,
    BLOCKED_INSUFFICIENT_LOCAL_DISK, BLOCKED_UNKNOWN_FOREIGN_EXPOSURE, BLOCKED_UNKNOWN_INVENTORY,
    BLOCKED_UNKNOWN_LOCAL_DISK, BLOCKED_UNKNOWN_PENDING_WRITES, BLOCKED_UNKNOWN_RUNTIME_ENVELOPE,
    MAX_ADMISSION_JSON_BYTES, MAX_IDENTITY_BYTES, admission_canonical_bytes, admission_digest,
    decide_admission, parse_admission_bytes,
};
pub use super::cache_archive::cache_archive_envelope::{
    ENVELOPE_SCHEMA, FrozenInnerPayload, HARD_CEILING_BYTES, MAX_ENVELOPE_JSON_BYTES,
    MAX_GENERATION_SLOTS, MAX_OUTER_BYTES, MIN_RESERVE_BYTES, MeasuredEnvelope,
    OPERATING_CEILING_BYTES, OUTER_MEMBER_NAME, OuterEnvelope, OuterEnvelopeIdentity,
    PreflightMeasurement, UNKNOWN_HOSTED_PACKAGING, UNKNOWN_REPACK_BASIS, UNKNOWN_RUNNER_LAYOUT,
    UNKNOWN_TOOL_IDENTITY, UnknownEnvelope, envelope_canonical_bytes, freeze_inner_payload,
    measure_envelope, parse_envelope_bytes, run_preflight_packaging, unknown_envelope,
};
pub use super::cache_archive::{
    ExtractedSnapshot, MAX_ARCHIVE_BYTES, MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_ENTRY_BYTES,
    MAX_ARCHIVE_TOTAL_BYTES, PackRequest, PackedArchive, extract_snapshot_archive,
    pack_snapshot_archive,
};
pub use super::cache_inventory::{
    EntryTupleV1, INVENTORY_SCHEMA, InventoryV1, MAX_CACHE_KEY_BYTES, MAX_CACHE_REF_BYTES,
    MAX_INVENTORY_ENTRIES, MAX_INVENTORY_JSON_BYTES, build_inventory, inventory_canonical_bytes,
    inventory_digest, mark_inventory_unknown, measured_bytes, parse_inventory_bytes, unknown_bytes,
    unknown_inventory_gap,
};
pub use super::cache_snapshot::{
    ArchivedProjectionEntry, LaneRoot, MANIFEST_ARCHIVE_MEMBER, MAX_SNAPSHOT_ENTRIES,
    MAX_SNAPSHOT_PAYLOAD_BYTES, PROJECTION_ARCHIVE_PREFIX, PROJECTION_INDEX_ARCHIVE_MEMBER,
    PlannedProjectionEntry, REQUIRED_OUTPUT_INDEX_ARCHIVE_MEMBER, SNAPSHOT_ARCHIVE_FORMAT,
    SNAPSHOT_MTIME_PRECISION, SNAPSHOT_SCHEMA, STATE_ARCHIVE_PREFIX, STATE_INDEX_ARCHIVE_MEMBER,
    SnapshotInputs, SnapshotManifestV1, SnapshotPlan, StateEntryV1, StateMtime,
    archived_projection_records, build_snapshot_plan, collect_state_index, digest_record,
    excluded_policy_digest, is_excluded, manifest_canonical_bytes, parse_manifest_bytes,
    parse_projection_index_bytes, parse_required_output_bytes, parse_state_index_bytes,
    plan_projection_payload, projection_index_digest, required_output_index_digest,
    state_index_digest, validate_required_outputs, validate_state_index,
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
pub use super::merge::{MergeContext, merge_internal, merge_internal_with_context};
pub use super::merge_request::assemble_merge_request;
pub use super::owned_tool_preview::{SourceQualificationTrigger, preview_owned_tool_candidates};
pub use super::pins::{acquire_script_argv, consumer_acquire_step_with_manifest};
pub use super::plan::{plan_text, plan_text_checked};
pub use super::plan_output_limits::{JOB_OUTPUTS_BUDGET_UTF16_BYTES, PlanOutputMode};
pub use super::prepare::{GenerationPreparation, prepare};
pub use super::preseed_manifest::{PRESEED_MANIFEST_OP, write_preseed_manifest};
pub use super::preview_producer::{
    ACQUIRE_RELEASE_LEAVES_OP, ADMIT_RELEASE_AGGREGATE_OP, EXECUTE_RELEASE_PRODUCER_OP,
    PREVIEW_CACHE_RECIPE_KEY, acquire_release_leaves, admit_release_aggregate,
    execute_release_producer,
};
pub use super::product_release_pins::cache_recipes::{
    CacheRecipe, CacheRecipeError, CacheRecipeKind, PREVIEW_PRODUCER_RECIPE_KEY, all_recipes,
    lookup_recipe, parse_kind, recipes_of, validate_catalog, validate_catalog_slice,
    validate_recipe,
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
pub use super::source_prep::cache_source_projection::{
    CheckoutState, EntryKind, InstallOutcome, ProjectionEntry, RealMtime, RestoreDecision,
    authenticate_entry, checkout_metadata_matches, decide, install_staged_file,
    install_staged_symlink, is_git_path, reconcile, reject_link_parents, restore_directory_times,
    restore_times, sha256_hex, sort_deepest_first, staged_path, validate_link_target,
    validate_link_target_in_root, validate_relative_path, validate_sha256, verify_times,
};
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

pub const ORCHESTRATOR_VERSION: u32 = 0;

pub use crate::qualification_candidate::qualify_tools;

pub use super::homebrew::qualification::execute_homebrew_qualification_from_env;
pub use super::homebrew::runtime::execute_native_homebrew_check_from_env;
pub(crate) use super::package_release_publish::{release_policy_admission, stable_tag_origin};

pub use super::image_maintenance::{
    BuildJobCandidateRequest, CheckJobSourceRequest, InstallBuildxRequest,
    ValidateJobCandidateRequest, build_job_candidate, check_job_source, install_buildx,
    validate_job_candidate,
};
pub use velnor_actions_mise::CheckDeadline;

#[path = "group_ledger/mod.rs"]
mod group_ledger;
pub use self::group_ledger::{
    ASSEMBLE_GROUP_LEDGERS_OP, EMIT_GROUP_LEDGER_OP, assemble_group_ledgers, emit_group_ledger,
};

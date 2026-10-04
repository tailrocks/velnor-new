//! Public contract facade, grouped separately from module registration.
pub use crate::archive::{ArchiveInputs, archive_id};
pub use crate::branch::is_valid_branch_name;
pub use crate::canonical::{
    CompatibilityInputs, Digest, StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity,
    TaskInput, canonical_json_bytes, canonical_json_str, compatibility_id, digest_b3, input_digest,
    is_valid_digest, normalize_posix_path, validate_digest,
};
pub use crate::closure::{ClosureBuilder, Provenance, TaskInputClosure};
pub use crate::compiled_support::CompiledSupportSource;
pub use crate::config::{
    DeclaredCompileDriver, DeclaredTestRunner, DiscoveryConfig, GeneratorValidation,
    NativeDesktopChecks, RequiredNativeObligation, RequiredNativeObligations, RequiredNativePhase,
    ResourcesConfig, RootProblem, RunnerSelection, RustConfiguration, RustFeatureMode,
    RustStackConfig, ShardTimingEvidence, StacksConfig, SwiftChecks, SwiftFfiArtifacts,
    SwiftInputs, SwiftTestFramework, TestShardingConfig, TofuStackConfig, Utf8RepoRelDir,
    VelnorConfig, VelnorSupportWorkflow, VerificationConfig, WorkflowConfig, WorkflowPolicy,
    WorkloadConfig, WorkloadKind, is_valid_custom_task_name, is_valid_feature_name,
    is_valid_rust_target, is_valid_workload_path, validate_shard_changes_need_evidence,
};
pub use crate::discover::{
    BUILTIN_EXCLUSIONS, DETECTION_SCHEMA, DetectError, DetectedProject, DetectionStatus,
    DetectorEntry, FileIndex, IGNORED_REASON, IndexError, IndexMode, Stack, apply_stack_ignores,
    build_index, build_index_from_list, build_index_from_tracked, build_index_walk,
    check_duplicates, is_excluded, matches_glob, reverse_closure, selected_projects,
    validate_pattern,
};
pub use crate::errors::{ContractError, sanitize_error_detail};
pub use crate::extensions::{
    RUST_EXTENSION_REQUIRED_SLOTS, TOFU_EXTENSION_REQUIRED_SLOTS, validate_rust_extension,
    validate_tofu_extension,
};
pub use crate::finding::Finding;
pub use crate::formats::{
    AGENTS_MD_PATH, CLAUDE_MD_PATH, CLAUDE_MD_TARGET, DECLARED_GITHUB_FORMATS, find_github_format,
    is_declared_github_format,
};
pub use crate::freshness::{
    FRESHNESS_CLASSES, FreshnessRequirement, RunnerImageEvidence, UNOBSERVED_IMAGE_VALUE,
    runner_family_changed, validate_freshness_class,
};
pub use crate::git_ref::is_valid_git_ref_fragment;
pub use crate::graph::{
    CachePolicy, EdgeKind, ResourceClass, ResourceDemand, TaskEdge, TaskGraph, TaskNode,
    validate_plan_edges,
};
pub use crate::ids::{
    artifact_id_for_baseline, artifact_id_for_crate_job, artifact_id_for_final,
    artifact_id_for_matrix, artifact_id_for_plan, manifest_key_for_cargo_manifest,
    matrix_id_for_task_group, matrix_key_for_id, plan_id_for_run, report_id_for_matrix,
    run_key_for_ci, split_shard_suffix, target_key, task_id_for_internal, task_id_for_stack,
    task_report_id_for_task, validate_artifact_id, validate_fetch_root, validate_id,
    validate_matrix_key, validate_plan_id, validate_report_id, validate_run_key, validate_task_id,
    validate_task_report_id,
};
pub use crate::manifest::{
    ActionPin, CandidateArtifactManifest, GeneratorBinary, GeneratorLock, LockedGenerator,
    MiseBootstrap, ReleaseManifest, TargetRecord, require_release_version,
};
pub use crate::marker::{
    MARKER_PREFIX, MARKER_SUFFIX, OLD_MARKER_PREFIX, generated_source, is_generated_marker_line,
    is_marker_version, marker_for_version,
};
pub use crate::policy::{
    FreshnessEntry, FreshnessStatus, GithubRunnerImages, NightlyRecord, PolicyException,
    RunnerInventory, VersionPolicy, days_between,
};
pub use crate::propose::{
    CandidateOutcome, IdentityInputs, ProposedTask, StackCandidate, check_candidate_outcomes,
    component_id_for_unit, project_root_for_unit_path,
};
pub use crate::secrets::is_secret_env_name;
pub use crate::strict_json::parse_strict_json;
pub use crate::targets::{
    EXPECTED_REPOSITORY, RELEASE_MANIFEST_FILENAME, SUPPORTED_TARGETS, asset_filename,
    check_release_artifact, is_seed_tag_for_version, is_supported_target, target_for_runner_label,
};
pub use crate::tool_targets::tool_target_for_runner_label;
pub use crate::tooling::ToolIdentity;
pub use crate::vcs::VcsInputs;
pub use crate::workflow::cache_receipt_recipe::{cache_producer_recipe_digest, cache_receipt_root};
pub use crate::workflow::mbx_export_descriptor::{
    MbxCacheDomain, MbxExportDescriptor, MbxOwnerIdentity,
};
pub use crate::workflow::mbx_producer::PureMbxProducer;
pub use crate::workflow::producer_inventory::{
    ProducerAdmission, ProducerEventContext, ProducerInventory, ProducerPolicy, ProducerRole,
};
pub use crate::workflow::source_helper::{
    CompiledSourceHelper, HelperInvocation, SOURCE_HELPER_ARGUMENT_BYTES_MAX, SourceBoundHelper,
    SourceBoundOperation, compiled_source_sha256,
};
pub use crate::workflow::source_producer::{
    ProducerTerminalError, SourceProducer, SourceProducerRole,
};
pub use crate::workflow::tool_producer::{PureToolProducer, ToolCacheDescriptor, ToolCacheDomain};
pub use crate::workflow::tool_producer_selection::{
    PLAN_CARGO_FALLBACK_CONDITION, PLAN_CARGO_FALLBACK_OUTPUT, ToolProducerSelection,
};
pub use crate::workflow::{
    ActionBinding, ActionOutcome, ActionOutput, ActionReport, BaselineProof, BaselineStatus,
    CANDIDATE_ATTESTATION_FILENAME, CANDIDATE_EVIDENCE_SUBDIR, CI_WORKFLOW_PATH,
    CRATE_JOB_ID_PREFIX, CacheLayer, CacheMode, CacheOutcome, CacheResult, CacheSnapshotDomain,
    CompiledNativeExecRecipe, CompiledRustReportRecipe, CompilerDriver, Concurrency, CrateJob,
    CrateObligation, EntryCacheIds, ExecuteTaskIds, ExecuteTaskRef, FINAL_JSON_FILENAME,
    FRESHNESS_CRON_WEEKLY, FRESHNESS_WORKFLOW_PATH, FinalCounts, FinalReport, FinalStatus,
    HelperObligationBinding, HelperObligationDescriptor, HelperObligationOutcome,
    HelperObligationReport, Job, JobConclusion, JobTimeout, MATRIX_JSON_FILENAME,
    ManifestTaskProof, MatrixEntry, MatrixReport, MatrixStatus, MatrixTaskEntry, NEEDS_CHANNEL_ENV,
    NEEDS_CHANNEL_EXPRESSION, NEEDS_EXPECTED_ENV, NativeCredentialScope,
    NativeValidationDescriptor, NeedsConclusions, NotSelectedReason, ObligationDecision,
    PLAN_DISPLAY_NAME, PLAN_JOB_ID, PLAN_JSON_FILENAME, PermissionLevel, Permissions, Plan,
    PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanPackage, PlanRunner,
    REQUIRED_CONDITION, REQUIRED_DISPLAY_NAME, REQUIRED_JOB_ID, RequiredCheckMigration,
    RequiredJobResult, RustCompilerOperation, RustCompilerTools, RustReportFrame,
    STALE_WORKFLOW_PATHS, ScheduleTrigger, Step, StepId, StepKind, TOFU_DISPLAY_PREFIX,
    TOFU_JOB_ID_PREFIX, TaskExecutionIdentity, TaskReport, TaskStatus, TaskTiming,
    TaskTimingSource, Trigger, Trust, ValidatorKind, VerificationScope, WORKFLOW_DISPLAY_NAME,
    WORKLOAD_DISPLAY_PREFIX, WORKLOAD_JOB_ID_PREFIX, WorkflowEvent, WorkflowIr,
    assign_crate_job_ids, check_matrix_agreement, crate_display_label, crate_display_name,
    final_report_id_for_run, final_report_relpath, is_crate_job_id, is_safe_display_name,
    join_runner_temp, matrix_json_bytes, matrix_report_relpath, plan_json_bytes,
    quote_literal_run_arg, slugify_segment, task_report_relpath, tofu_display_name,
    trust_for_event, validate_final_report_id, validate_job_id, validate_matrix_run,
    workload_display_name,
};

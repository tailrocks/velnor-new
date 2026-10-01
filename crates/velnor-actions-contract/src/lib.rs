//! Stack-neutral workflow/task contracts.
//!
//! Owns task graphs, identities, reports, and recommendations. Must not own
//! Rust/Cargo, Mise, process, filesystem, YAML, CLI, or generic app models.
//!
//! All types here are effect-free data plus pure derivation/validation.
//! Derivation formulas are normative; example strings in docs are illustrative.

pub mod archive;
pub mod cachekey;
pub mod candidate_manifest;
pub mod canonical;
pub mod config;
pub mod errors;
pub mod extensions;
pub mod finding;
pub mod formats;
pub mod freshness;
pub mod graph;
pub mod ids;
pub mod manifest;
pub mod marker;
pub mod policy;
pub mod secrets;
pub mod strict_json;
pub mod targets;
pub mod tooling;
pub mod vcs;
pub mod workflow;

pub use archive::{ArchiveInputs, archive_id};
pub use canonical::{
    CompatibilityInputs, Digest, StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity,
    TaskInput, canonical_json_bytes, canonical_json_str, compatibility_id, digest_b3, input_digest,
    is_valid_digest, normalize_posix_path, validate_digest,
};
pub use config::{
    DeclaredCompileDriver, DeclaredTestRunner, DiscoveryConfig, GeneratorValidation,
    ResourcesConfig, RunnerSelection, RustConfiguration, RustStackConfig, ShardTimingEvidence,
    StacksConfig, TestShardingConfig, VelnorConfig, VelnorSupportWorkflow, WorkflowConfig,
    WorkflowPolicy, is_valid_custom_task_name, is_valid_feature_name, is_valid_rust_target,
    validate_shard_changes_need_evidence,
};
pub use errors::{ContractError, sanitize_error_detail};
pub use extensions::{RUST_EXTENSION_REQUIRED_SLOTS, validate_rust_extension};
pub use finding::Finding;
pub use formats::{DECLARED_GITHUB_FORMATS, find_github_format, is_declared_github_format};
pub use freshness::{
    FRESHNESS_CLASSES, FreshnessRequirement, RunnerImageEvidence, runner_family_changed,
    validate_freshness_class,
};
pub use graph::{
    CachePolicy, EdgeKind, ResourceClass, ResourceDemand, TaskEdge, TaskGraph, TaskNode,
    validate_plan_edges,
};
pub use ids::{
    artifact_id_for_baseline, artifact_id_for_crate_job, artifact_id_for_final,
    artifact_id_for_matrix, artifact_id_for_plan, manifest_key_for_cargo_manifest,
    matrix_id_for_task_group, matrix_key_for_id, plan_id_for_run, report_id_for_matrix,
    run_key_for_ci, split_shard_suffix, target_key, task_id_for_internal, task_id_for_stack,
    task_report_id_for_task, validate_artifact_id, validate_id, validate_matrix_key,
    validate_plan_id, validate_report_id, validate_run_key, validate_task_id,
    validate_task_report_id,
};
pub use manifest::{
    ActionPin, CandidateArtifactManifest, GeneratorBinary, GeneratorLock, LockedGenerator,
    MiseBootstrap, ReleaseManifest, TargetRecord, require_release_version,
};
pub use marker::{MARKER_PREFIX, OLD_MARKER_PREFIX, is_generated_marker_line};
pub use policy::{
    FreshnessEntry, FreshnessStatus, GithubRunnerImages, NightlyRecord, PolicyException,
    RunnerInventory, VersionPolicy, days_between,
};
pub use secrets::is_secret_env_name;
pub use strict_json::parse_strict_json;
pub use targets::{
    EXPECTED_REPOSITORY, RELEASE_MANIFEST_FILENAME, SUPPORTED_TARGETS, asset_filename,
    check_release_artifact, is_seed_tag_for_version, is_supported_target, target_for_runner_label,
};
pub use tooling::ToolIdentity;
pub use vcs::VcsInputs;
pub use workflow::{
    BaselineProof, BaselineStatus, CANDIDATE_ATTESTATION_FILENAME, CANDIDATE_EVIDENCE_SUBDIR,
    CI_WORKFLOW_PATH, CRATE_JOB_ID_PREFIX, CacheLayer, CacheOutcome, CacheResult, Concurrency,
    CrateJob, CrateObligation, EntryCacheIds, ExecuteTaskIds, ExecuteTaskRef, FINAL_JSON_FILENAME,
    FRESHNESS_CRON_WEEKLY, FRESHNESS_WORKFLOW_PATH, FinalCounts, FinalReport, FinalStatus, Job,
    JobConclusion, MATRIX_JSON_FILENAME, ManifestTaskProof, MatrixEntry, MatrixReport,
    MatrixStatus, MatrixTaskEntry, NEEDS_CHANNEL_ENV, NEEDS_CHANNEL_EXPRESSION, NEEDS_EXPECTED_ENV,
    NeedsConclusions, NotSelectedReason, ObligationDecision, PLAN_DISPLAY_NAME, PLAN_JOB_ID,
    PLAN_JSON_FILENAME, PermissionLevel, Permissions, Plan, PlanBaseline, PlanGenerator,
    PlanMatrix, PlanObligation, PlanPackage, PlanRunner, REQUIRED_CONDITION, REQUIRED_DISPLAY_NAME,
    REQUIRED_JOB_ID, RequiredCheckMigration, RequiredJobResult, STALE_WORKFLOW_PATHS,
    ScheduleTrigger, Step, StepKind, TaskReport, TaskStatus, TaskTiming, Trigger, Trust,
    ValidatorKind, WORKFLOW_DISPLAY_NAME, WorkflowEvent, WorkflowIr, assign_crate_job_ids,
    check_matrix_agreement, crate_display_label, crate_display_name, final_report_id_for_run,
    final_report_relpath, is_safe_display_name, join_runner_temp, matrix_json_bytes,
    matrix_report_relpath, plan_json_bytes, slugify_segment, task_report_relpath, trust_for_event,
    validate_final_report_id, validate_job_id, validate_matrix_run,
};

/// Version marker for the contract schema shell.
pub const CONTRACT_VERSION: u32 = 0;

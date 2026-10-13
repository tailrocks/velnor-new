//! Stack-neutral workflow/task contracts.
//!
//! Owns task graphs, identities, reports, and recommendations. Must not own
//! Rust/Cargo, Mise, process, filesystem, YAML, CLI, or generic app models.
//!
//! All types here are effect-free data plus pure derivation/validation.
//! Derivation formulas are normative; example strings in docs are illustrative.

pub mod cachekey;
pub mod candidate_manifest;
pub mod canonical;
pub mod config;
pub mod errors;
pub mod ids;
pub mod manifest;
pub mod marker;
pub mod targets;
pub mod workflow;

pub use canonical::{
    CompatibilityInputs, Digest, StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity,
    TaskInput, canonical_json_bytes, canonical_json_str, compatibility_id, digest_b3, input_digest,
    is_valid_digest, normalize_posix_path, validate_digest,
};
pub use config::{
    DiscoveryConfig, GeneratorValidation, PolicyJob, ResourcesConfig, RunnerSelection,
    RustConfiguration, RustStackConfig, StacksConfig, TestShardingConfig, VelnorConfig,
    VelnorSupportWorkflow, WorkflowConfig, WorkflowPolicy,
};
pub use errors::ContractError;
pub use ids::{
    artifact_id_for_candidate, artifact_id_for_final, artifact_id_for_matrix, artifact_id_for_plan,
    manifest_key_for_cargo_manifest, matrix_id_for_task_group, matrix_key_for_id, plan_id_for_run,
    report_id_for_matrix, run_key_for_ci, target_key, task_id_for_internal, task_id_for_stack,
    task_report_id_for_task, validate_artifact_id, validate_id, validate_matrix_key,
    validate_plan_id, validate_report_id, validate_run_key, validate_task_id,
    validate_task_report_id,
};
pub use manifest::{
    ActionPin, CandidateArtifactManifest, GeneratorBinary, GeneratorLock, LockedGenerator,
    MiseBootstrap, ReleaseManifest, TargetRecord,
};
pub use marker::MARKER_PREFIX;
pub use targets::{
    RELEASE_MANIFEST_FILENAME, SUPPORTED_TARGETS, asset_filename, is_supported_target,
    target_for_runner_label,
};
pub use workflow::{
    BaselineProof, BaselineStatus, CacheLayer, CacheOutcome, CacheResult, CandidateReport,
    CandidateStatus, Concurrency, ExecuteTaskIds, ExecuteTaskRef, FinalCounts, FinalReport,
    FinalStatus, Job, MatrixEntry, MatrixReport, MatrixStatus, MatrixTaskEntry, NotSelectedReason,
    ObligationDecision, Permissions, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation,
    PlanPackage, PlanRunner, RequiredJobResult, Step, StepKind, TaskReport, TaskStatus, Trigger,
    Trust, WorkflowEvent, WorkflowIr, candidate_report_id_for_run, final_report_id_for_run,
    validate_candidate_report_id, validate_final_report_id,
};

/// Version marker for the contract schema shell.
pub const CONTRACT_VERSION: u32 = 0;

//! Stack-neutral workflow IR, matrix, plan, and report types.
pub mod artifacts;
pub mod baseline;
pub mod cache_ids;
pub mod crate_job;
pub mod dispatch;
pub mod execute;
pub mod ir;
pub mod jobs;
pub mod lanes;
pub mod matrix_entry;
pub mod named_check_lanes;
pub mod needs;
pub mod permissions;
pub mod plan;
pub mod platform;
pub mod qualification;
pub mod qualification_cache_lineage;
pub mod qualification_dispatch;
pub mod qualification_phase;
pub mod report;
mod report_validate;
pub mod step;
pub mod step_identity;
mod step_mbx_lifecycle;
mod step_protocol;
pub mod task_execution_manifest;
pub mod timeout;
pub mod trust;
pub use artifacts::{
    CANDIDATE_ATTESTATION_FILENAME, CANDIDATE_EVIDENCE_SUBDIR, FINAL_JSON_FILENAME,
    MATRIX_JSON_FILENAME, PLAN_JSON_FILENAME, check_matrix_agreement, matrix_json_bytes,
    plan_json_bytes,
};
pub use baseline::{BaselineProof, BaselineStatus, ManifestTaskProof, PlanBaseline};
pub use cache_ids::EntryCacheIds;
pub use crate_job::{CrateJob, CrateObligation};
pub use dispatch::{DispatchInput, DispatchInputType, WorkflowDispatch};
pub use execute::{ExecuteTaskIds, ExecuteTaskRef};
pub use ir::{Concurrency, Job, ScheduleTrigger, Trigger, WorkflowIr};
pub use jobs::{
    CI_WORKFLOW_PATH, CRATE_JOB_ID_PREFIX, FRESHNESS_CRON_WEEKLY, FRESHNESS_WORKFLOW_PATH,
    PLAN_DISPLAY_NAME, PLAN_JOB_ID, REQUIRED_CONDITION, REQUIRED_DISPLAY_NAME, REQUIRED_JOB_ID,
    STALE_WORKFLOW_PATHS, TOFU_APPLY_WORKFLOW_PATH, TOFU_DISPLAY_PREFIX, TOFU_JOB_ID_PREFIX,
    ValidatorKind, WORKFLOW_DISPLAY_NAME, assign_crate_job_ids, crate_display_label,
    crate_display_name, is_crate_job_id, is_safe_display_name, slugify_segment, tofu_display_name,
    validate_job_id,
};
pub use lanes::{
    HOSTED_SUFFIX, LaneClass, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV,
    NAMED_CHECK_LANES_ENV, NamedCheckLane, NamedCheckLaneVariant, SCALE_SUFFIX, expand_workflow,
    lane_class, named_check_lanes,
};
pub use needs::{
    NEEDS_CHANNEL_ENV, NEEDS_CHANNEL_EXPRESSION, NEEDS_EXPECTED_ENV, NeedsConclusions,
    RequiredCheckMigration,
};
pub use permissions::{PermissionLevel, Permissions};
pub use plan::{
    DYNAMIC_MATRIX_OUTPUT_MODE, MatrixEntry, ObligationDecision, PLAN_MATRIX_OUTPUT_MODE_ENV, Plan,
    PlanGenerator, PlanMatrix, PlanObligation, PlanPackage, PlanRunner, WorkflowEvent,
    validate_matrix_run,
};
pub use platform::{
    PlannedPlatform, PlannedRunnerEnvironment, PlatformBinding, PlatformRunnerEnvironment,
    PlatformUnavailableReason,
};
pub use qualification::{
    FinalCounts, FinalReport, FinalStatus, JobConclusion, RequiredJobResult,
    final_report_id_for_run, final_report_relpath, join_runner_temp, matrix_report_relpath,
    task_report_relpath, validate_final_report_id,
};
pub use qualification_cache_lineage::{
    BoundQualificationCacheKeys, MAX_QUALIFICATION_CACHE_LANES, MAX_QUALIFICATION_RECEIPT_BYTES,
    MAX_QUALIFICATION_RECEIPT_DEPTH, QUALIFICATION_CACHE_DIRECTIVES_OUTPUT,
    QUALIFICATION_CACHE_RECEIPT_ARTIFACT, QUALIFICATION_CACHE_RECEIPT_FILENAME,
    QualificationCacheAdmission, QualificationCacheArtifact, QualificationCacheBackendEntry,
    QualificationCacheBackendObservation, QualificationCacheDirective,
    QualificationCacheLaneDirective, QualificationCacheLaneReceipt, QualificationCacheLayer,
    QualificationCacheLayerDirective, QualificationCacheLayerReceipt,
    QualificationCacheProducerContext, QualificationCacheReceipt,
    QualificationCacheReceiptArtifactDocument, QualificationCacheReceiptLink,
    QualificationCacheRestore, QualificationCacheRestoreDirective,
    QualificationCacheRestoreExpectation, QualificationCacheRestorePolicy,
    QualificationCacheRestoreResult, QualificationCacheRunMetadata, QualificationCacheSave,
    QualificationCacheSaveActionResult, QualificationCacheSavePolicy, QualificationCacheSlot,
    QualificationRuntimeIdentity, QualificationRuntimeIdentityField,
    QualificationRuntimeIdentityRequirements, QualificationRuntimePlatform,
    QualificationSourceDelta,
};
pub use qualification_dispatch::{QualificationDispatch, QualificationRunRef};
pub use qualification_phase::{
    QUALIFICATION_CACHE_ENABLED_OUTPUT, QUALIFICATION_CACHE_WRITE_OUTPUT,
    QUALIFICATION_CAMPAIGN_OUTPUT, QUALIFICATION_PHASE_OUTPUT, QualificationPhase,
};
pub use report::{
    CacheLayer, CacheOutcome, CacheResult, MatrixReport, MatrixStatus, MatrixTaskEntry,
    NotSelectedReason, TaskReport, TaskStatus, TaskTiming,
};
pub use step::{
    MAX_TASK_EXECUTION_ARGV, MAX_TASK_EXECUTION_ENV, Step, StepKind, TASK_COVERED_OUTPUT,
    task_execution_condition,
};
pub use step_identity::{MBX_WORKSPACE_CLEAN_CONDITION, StepId, StepRole};
pub use task_execution_manifest::{
    MAX_TASK_EXECUTION_FRAME_BYTES, MAX_TASK_EXECUTION_RECORDS, TASK_EXECUTION_FRAME_MAGIC,
    TASK_EXECUTION_MANIFEST_PATH, TASK_EXECUTION_MANIFEST_SCHEMA, TaskExecutionManifestEntryV1,
    TaskExecutionManifestV1,
};
pub use timeout::JobTimeout;
pub use trust::{Trust, trust_for_event};

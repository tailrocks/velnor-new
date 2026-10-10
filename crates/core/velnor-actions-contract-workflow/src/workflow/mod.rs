//! Stack-neutral workflow IR, matrix, plan, and report types.
pub mod artifact_build;
pub mod artifacts;
pub mod baseline;
pub mod cache_ids;
pub mod crate_job;
pub mod execute;
pub mod ir;
pub mod job_output;
pub mod jobs;
pub mod lanes;
pub mod matrix_entry;
pub mod named_check_lanes;
pub mod needs;
pub mod permissions;
pub mod plan;
pub mod qualification;
pub mod report;
pub mod reusable_callee;
pub mod runtime_receipt;
pub mod step;
pub mod step_identity;
mod step_protocol;
pub mod timeout;
pub mod trust;
pub use artifact_build::{
    ARTIFACT_BUILD_OBSERVATIONS_FILENAME, ARTIFACT_BUILD_OUTPUTS_DIRECTORY,
    ARTIFACT_BUILD_RESULT_FILENAME, ARTIFACT_EXPORT_OPERATION, ARTIFACT_HOSTED_MATRIX_OUTPUT,
    ARTIFACT_MATRIX_MAX_PARALLEL_ENV, ARTIFACT_MATRIX_NEEDS_JOB_ENV, ARTIFACT_MATRIX_PROVIDER_ENV,
    ARTIFACT_MISE_TASK_ENV, ARTIFACT_NAME_ENV, ARTIFACT_PLAN_DIGEST_ENV, ARTIFACT_PROVIDER_ENV,
    ARTIFACT_SOURCE_SHA_ENV, ARTIFACT_TASK_ID_ENV, ARTIFACT_VELNOR_MATRIX_OUTPUT,
    ArtifactBuildExpectation, ArtifactBuildFile, ArtifactBuildIdentity, ArtifactBuildMatrix,
    ArtifactBuildMatrixEntry, ArtifactBuildObservation, ArtifactBuildProducer,
    ArtifactBuildProvider, ArtifactBuildResult, ArtifactBuildRunContext, ArtifactBuildTaskPlan,
    DownloadedArtifactOutput, VERIFICATION_ARTIFACT_EXPORT_OPERATION, artifact_matrix_for_provider,
    artifact_name, artifact_plan_providers, canonical_plan_digest, expected_artifact_builds,
    export_artifact_result, reconcile_artifact_builds,
};
pub use artifacts::{
    CANDIDATE_ATTESTATION_FILENAME, CANDIDATE_EVIDENCE_SUBDIR, FINAL_JSON_FILENAME,
    MATRIX_JSON_FILENAME, PLAN_JSON_FILENAME, check_matrix_agreement, matrix_json_bytes,
    plan_json_bytes,
};
pub use baseline::{BaselineProof, BaselineStatus, ManifestTaskProof, PlanBaseline};
pub use cache_ids::EntryCacheIds;
pub use crate_job::{CrateJob, CrateObligation};
pub use execute::{ExecuteTaskIds, ExecuteTaskRef};
pub use ir::{Concurrency, Job, Trigger, WorkflowIr};
pub use job_output::{JobOutput, JobOutputName, JobOutputSource};
pub use jobs::{
    CI_WORKFLOW_PATH, FRESHNESS_CRON_WEEKLY, FRESHNESS_WORKFLOW_PATH, PLAN_DISPLAY_NAME,
    PLAN_JOB_ID, REQUIRED_CONDITION, REQUIRED_DISPLAY_NAME, REQUIRED_JOB_ID,
    RequiredCheckMigration, STALE_WORKFLOW_PATHS, ScheduleTrigger, TOFU_DISPLAY_PREFIX,
    WORKFLOW_DISPLAY_NAME, crate_display_label, crate_display_name, is_safe_display_name,
    tofu_display_name,
};
pub use lanes::{
    EXECUTION_MODE_ENV, HOSTED_SUFFIX, LaneClass, NAMED_CHECK_JOB_ID_ENV,
    NAMED_CHECK_LANE_VARIANT_ENV, NAMED_CHECK_LANES_ENV, NamedCheckLane, NamedCheckLaneVariant,
    SCALE_SUFFIX, expand_workflow, lane_class, named_check_lanes, verification_job_mode,
};
pub use matrix_entry::MatrixEntry;
pub use needs::{
    NEEDS_CHANNEL_ENV, NEEDS_CHANNEL_EXPRESSION, NEEDS_EXPECTED_ENV, NeedsConclusions,
    TASK_REPORT_PRODUCERS_EXPECTED_ENV, TaskReportProducerInventory,
};
pub use permissions::{PermissionLevel, Permissions};
pub use plan::{
    DYNAMIC_MATRIX_OUTPUT_MODE, ObligationDecision, PLAN_MATRIX_OUTPUT_MODE_ENV, Plan,
    PlanGenerator, PlanMatrix, PlanObligation, PlanPackage, PlanRunner, WorkflowEvent,
    validate_matrix_run,
};
pub use qualification::{
    FinalCounts, FinalReport, FinalStatus, JobConclusion, RequiredJobResult,
    final_report_id_for_run, final_report_relpath, join_runner_temp, matrix_report_relpath,
    task_report_relpath, validate_final_report_id,
};
pub use report::{
    CacheLayer, CacheOutcome, CacheResult, MatrixReport, MatrixStatus, MatrixTaskEntry,
    NotSelectedReason, TaskReport, TaskStatus, TaskTiming,
};
pub use reusable_callee::{
    REUSABLE_CALLEE_EVENT, REUSABLE_CALLEE_GUARD_JOB, REUSABLE_CALLEE_INPUT_TYPE,
    REUSABLE_CALLEE_INPUTS, REUSABLE_CALLEE_WRITE_JOB, REUSABLE_CALLER_PERMISSIONS,
    ReusableCalleeContract, ReusableCalleeContractError, ReusableCalleeIdentity,
    ReusableCalleeIdentityError, ReusableCalleeInput, ReusableCalleePolicy,
    ReusableCalleePolicyError, schema_value,
};
pub use runtime_receipt::{
    TASK_RUNTIME_RECEIPTS_DIRECTORY, TaskRuntimeIdentity, TaskRuntimeReceipt,
};
pub use step::{Step, StepKind};
pub use step_identity::{StepId, StepRole};
pub use timeout::JobTimeout;
pub use trust::{Trust, trust_for_event};

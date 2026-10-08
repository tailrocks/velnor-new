//! Stack-neutral workflow IR, matrix, plan, and report types.
//!
//! Owns workflow intermediate representation, job and lane models, plan
//! construction, matrix entries, execution records, and final reports.
//! Must not own identities, configuration, release manifests,
//! detection, or proposals. Built on `velnor-actions-contract`
//! (identifiers), `velnor-actions-contract-release` (targets),
//! `velnor-actions-contract-config` (runners, execution), and
//! `velnor-actions-contract-planning` (task graphs).

pub mod workflow;

pub use workflow::{
    ARTIFACT_BUILD_OBSERVATIONS_FILENAME, ARTIFACT_BUILD_OUTPUTS_DIRECTORY,
    ARTIFACT_BUILD_RESULT_FILENAME, ARTIFACT_EXPORT_OPERATION, ARTIFACT_HOSTED_MATRIX_OUTPUT,
    ARTIFACT_MATRIX_MAX_PARALLEL_ENV, ARTIFACT_MATRIX_NEEDS_JOB_ENV, ARTIFACT_MATRIX_PROVIDER_ENV,
    ARTIFACT_MISE_TASK_ENV, ARTIFACT_NAME_ENV, ARTIFACT_PLAN_DIGEST_ENV, ARTIFACT_PROVIDER_ENV,
    ARTIFACT_SOURCE_SHA_ENV, ARTIFACT_TASK_ID_ENV, ARTIFACT_VELNOR_MATRIX_OUTPUT,
    ArtifactBuildExpectation, ArtifactBuildFile, ArtifactBuildIdentity, ArtifactBuildMatrix,
    ArtifactBuildMatrixEntry, ArtifactBuildObservation, ArtifactBuildProvider, ArtifactBuildResult,
    ArtifactBuildRunContext, ArtifactBuildTaskPlan, BaselineProof, BaselineStatus,
    CANDIDATE_ATTESTATION_FILENAME, CANDIDATE_EVIDENCE_SUBDIR, CI_WORKFLOW_PATH, CacheLayer,
    CacheOutcome, CacheResult, Concurrency, CrateJob, CrateObligation, DYNAMIC_MATRIX_OUTPUT_MODE,
    DownloadedArtifactOutput, EXECUTION_MODE_ENV, EntryCacheIds, ExecuteTaskIds, ExecuteTaskRef,
    FINAL_JSON_FILENAME, FRESHNESS_CRON_WEEKLY, FRESHNESS_WORKFLOW_PATH, FinalCounts, FinalReport,
    FinalStatus, HOSTED_SUFFIX, Job, JobConclusion, JobOutput, JobOutputName, JobOutputSource,
    JobTimeout, MATRIX_JSON_FILENAME, ManifestTaskProof, MatrixEntry, MatrixReport, MatrixStatus,
    MatrixTaskEntry, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV, NAMED_CHECK_LANES_ENV,
    NEEDS_CHANNEL_ENV, NEEDS_CHANNEL_EXPRESSION, NEEDS_EXPECTED_ENV, NamedCheckLane,
    NamedCheckLaneVariant, NeedsConclusions, NotSelectedReason, ObligationDecision,
    PLAN_DISPLAY_NAME, PLAN_JOB_ID, PLAN_JSON_FILENAME, PLAN_MATRIX_OUTPUT_MODE_ENV,
    PermissionLevel, Permissions, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation,
    PlanPackage, PlanRunner, REQUIRED_CONDITION, REQUIRED_DISPLAY_NAME, REQUIRED_JOB_ID,
    RequiredCheckMigration, RequiredJobResult, SCALE_SUFFIX, STALE_WORKFLOW_PATHS, ScheduleTrigger,
    Step, StepId, StepKind, StepRole, TASK_RUNTIME_RECEIPTS_DIRECTORY, TOFU_DISPLAY_PREFIX,
    TaskReport, TaskRuntimeIdentity, TaskRuntimeReceipt, TaskStatus, TaskTiming, Trigger, Trust,
    WORKFLOW_DISPLAY_NAME, WorkflowEvent, WorkflowIr, artifact_matrix_for_provider, artifact_name,
    canonical_plan_digest, check_matrix_agreement, crate_display_label, crate_display_name,
    expand_workflow, expected_artifact_builds, export_artifact_result, final_report_id_for_run,
    final_report_relpath, is_safe_display_name, join_runner_temp, matrix_json_bytes,
    matrix_report_relpath, named_check_lanes, plan_json_bytes, reconcile_artifact_builds,
    task_report_relpath, tofu_display_name, trust_for_event, validate_final_report_id,
    validate_matrix_run,
};

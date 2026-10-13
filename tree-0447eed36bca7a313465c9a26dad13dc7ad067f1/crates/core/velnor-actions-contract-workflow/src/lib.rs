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
    BaselineProof, BaselineStatus, CANDIDATE_ATTESTATION_FILENAME, CANDIDATE_EVIDENCE_SUBDIR,
    CI_WORKFLOW_PATH, CacheLayer, CacheOutcome, CacheResult, Concurrency, CrateJob,
    CrateObligation, DYNAMIC_MATRIX_OUTPUT_MODE, EntryCacheIds, ExecuteTaskIds, ExecuteTaskRef,
    FINAL_JSON_FILENAME, FRESHNESS_CRON_WEEKLY, FRESHNESS_WORKFLOW_PATH, FinalCounts, FinalReport,
    FinalStatus, HOSTED_SUFFIX, Job, JobConclusion, JobTimeout, MATRIX_JSON_FILENAME,
    ManifestTaskProof, MatrixEntry, MatrixReport, MatrixStatus, MatrixTaskEntry,
    NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV, NAMED_CHECK_LANES_ENV, NEEDS_CHANNEL_ENV,
    NEEDS_CHANNEL_EXPRESSION, NEEDS_EXPECTED_ENV, NamedCheckLane, NamedCheckLaneVariant,
    NeedsConclusions, NotSelectedReason, ObligationDecision, PLAN_DISPLAY_NAME, PLAN_JOB_ID,
    PLAN_JSON_FILENAME, PLAN_MATRIX_OUTPUT_MODE_ENV, PermissionLevel, Permissions, Plan,
    PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanPackage, PlanRunner,
    REQUIRED_CONDITION, REQUIRED_DISPLAY_NAME, REQUIRED_JOB_ID, RequiredCheckMigration,
    RequiredJobResult, SCALE_SUFFIX, STALE_WORKFLOW_PATHS, ScheduleTrigger, Step, StepId, StepKind,
    StepRole, TOFU_DISPLAY_PREFIX, TaskReport, TaskStatus, TaskTiming, Trigger, Trust,
    WORKFLOW_DISPLAY_NAME, WorkflowEvent, WorkflowIr, check_matrix_agreement, crate_display_label,
    crate_display_name, expand_workflow, final_report_id_for_run, final_report_relpath,
    is_safe_display_name, join_runner_temp, matrix_json_bytes, matrix_report_relpath,
    named_check_lanes, plan_json_bytes, task_report_relpath, tofu_display_name, trust_for_event,
    validate_final_report_id, validate_matrix_run,
};

//! Stack-neutral workflow IR, matrix, plan, and report types.
pub mod artifacts;
pub mod baseline;
pub mod cache_ids;
pub mod crate_job;
pub mod execute;
pub mod ir;
pub mod jobs;
pub mod lanes;
pub mod matrix_entry;
pub mod named_check_lanes;
pub mod needs;
pub mod permissions;
pub mod plan;
pub mod qualification;
pub mod report;
pub mod step;
pub mod step_identity;
mod step_protocol;
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
pub use execute::{ExecuteTaskIds, ExecuteTaskRef};
pub use ir::{Concurrency, Job, Trigger, WorkflowIr};
pub use jobs::{
    CI_WORKFLOW_PATH, FRESHNESS_CRON_WEEKLY, FRESHNESS_WORKFLOW_PATH, PLAN_DISPLAY_NAME,
    PLAN_JOB_ID, REQUIRED_CONDITION, REQUIRED_DISPLAY_NAME, REQUIRED_JOB_ID,
    RequiredCheckMigration, STALE_WORKFLOW_PATHS, ScheduleTrigger, TOFU_DISPLAY_PREFIX,
    WORKFLOW_DISPLAY_NAME, crate_display_label, crate_display_name, is_safe_display_name,
    tofu_display_name,
};
pub use lanes::{
    HOSTED_SUFFIX, LaneClass, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV,
    NAMED_CHECK_LANES_ENV, NamedCheckLane, NamedCheckLaneVariant, SCALE_SUFFIX, expand_workflow,
    lane_class, named_check_lanes,
};
pub use matrix_entry::MatrixEntry;
pub use needs::{
    NEEDS_CHANNEL_ENV, NEEDS_CHANNEL_EXPRESSION, NEEDS_EXPECTED_ENV, NeedsConclusions,
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
pub use step::{Step, StepKind};
pub use step_identity::{StepId, StepRole};
pub use timeout::JobTimeout;
pub use trust::{Trust, trust_for_event};

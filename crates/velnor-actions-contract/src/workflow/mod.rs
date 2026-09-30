//! Stack-neutral workflow IR, matrix, plan, and report types.
pub mod artifacts;
pub mod baseline;
pub mod cache_ids;
pub mod crate_job;
pub mod execute;
pub mod ir;
pub mod jobs;
pub mod plan;
pub mod qualification;
pub mod report;
pub use artifacts::{
    FINAL_JSON_FILENAME, MATRIX_JSON_FILENAME, PLAN_JSON_FILENAME, check_matrix_agreement,
    matrix_json_bytes, plan_json_bytes,
};
pub use baseline::{BaselineProof, BaselineStatus, ManifestTaskProof, PlanBaseline};
pub use cache_ids::EntryCacheIds;
pub use crate_job::{CrateJob, CrateObligation};
pub use execute::{ExecuteTaskIds, ExecuteTaskRef};
pub use ir::{Concurrency, Job, Permissions, Step, StepKind, Trigger, WorkflowIr};
pub use jobs::{
    CI_WORKFLOW_PATH, CRATE_JOB_ID_PREFIX, FRESHNESS_CRON_WEEKLY, FRESHNESS_WORKFLOW_PATH,
    NEEDS_CHANNEL_ENV, NEEDS_CHANNEL_EXPRESSION, NeedsConclusions, PLAN_DISPLAY_NAME, PLAN_JOB_ID,
    REQUIRED_CONDITION, REQUIRED_DISPLAY_NAME, REQUIRED_JOB_ID, RequiredCheckMigration,
    STALE_WORKFLOW_PATHS, ScheduleTrigger, ValidatorKind, WORKFLOW_DISPLAY_NAME,
    assign_crate_job_ids, crate_display_label, crate_display_name, slugify_segment,
    validate_job_id,
};
pub use plan::{
    MatrixEntry, ObligationDecision, Plan, PlanGenerator, PlanMatrix, PlanObligation, PlanPackage,
    PlanRunner, Trust, WorkflowEvent, validate_matrix_run,
};
pub use qualification::{
    CandidateReport, CandidateStatus, FinalCounts, FinalReport, FinalStatus, RequiredJobResult,
    candidate_report_id_for_run, final_report_id_for_run, final_report_relpath, join_runner_temp,
    matrix_report_relpath, task_report_relpath, validate_candidate_report_id,
    validate_final_report_id,
};
pub use report::{
    CacheLayer, CacheOutcome, CacheResult, MatrixReport, MatrixStatus, MatrixTaskEntry,
    NotSelectedReason, TaskReport, TaskStatus, TaskTiming,
};

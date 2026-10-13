//! Stack-neutral workflow IR, matrix, plan, and report types.
pub mod artifacts;
pub mod baseline;
pub mod cache_ids;
pub mod execute;
pub mod ir;
pub mod plan;
pub mod qualification;
pub mod report;
pub use artifacts::{
    FINAL_JSON_FILENAME, MATRIX_JSON_FILENAME, PLAN_JSON_FILENAME, check_matrix_agreement,
    matrix_json_bytes, plan_json_bytes,
};
pub use baseline::{BaselineProof, BaselineStatus, ManifestTaskProof, PlanBaseline};
pub use cache_ids::EntryCacheIds;
pub use execute::{ExecuteTaskIds, ExecuteTaskRef};
pub use ir::{Concurrency, Job, Permissions, Step, StepKind, Trigger, WorkflowIr};
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

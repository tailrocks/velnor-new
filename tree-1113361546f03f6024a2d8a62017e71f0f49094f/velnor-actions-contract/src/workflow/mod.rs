//! Stack-neutral workflow IR, matrix, plan, and report types.
pub mod baseline;
pub mod ir;
pub mod plan;
pub mod qualification;
pub mod report;
pub use baseline::{BaselineProof, BaselineStatus, PlanBaseline};
pub use ir::{Concurrency, Job, Permissions, Step, StepKind, Trigger, WorkflowIr};
pub use plan::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanGenerator,
    PlanMatrix, PlanObligation, PlanPackage, PlanRunner, Trust, WorkflowEvent,
};
pub use qualification::{
    CandidateReport, CandidateStatus, FinalCounts, FinalReport, FinalStatus, RequiredJobResult,
    candidate_report_id_for_run, final_report_id_for_run, validate_candidate_report_id,
    validate_final_report_id,
};
pub use report::{
    CacheLayer, CacheOutcome, CacheResult, MatrixReport, MatrixStatus, MatrixTaskEntry,
    NotSelectedReason, TaskReport, TaskStatus,
};

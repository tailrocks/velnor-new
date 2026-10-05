//! Contract plan, report, and workflow cases.
use crate::impl_contract_ids::{MANIFEST, TASK, sample_entry};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    BaselineProof, CacheLayer, CacheOutcome, CacheResult, Concurrency, ContractError, FinalCounts,
    FinalReport, FinalStatus, Job, JobConclusion, JobTimeout, MatrixReport, MatrixStatus,
    NotSelectedReason, ObligationDecision, Permissions, Plan, PlanBaseline, PlanGenerator,
    PlanMatrix, PlanObligation, PlanPackage, PlanRunner, RequiredJobResult, RunnerSelection, Step,
    StepKind, TaskReport, TaskStatus, Trigger, Trust, WorkflowEvent, WorkflowIr,
    artifact_id_for_matrix, artifact_id_for_plan, canonical_json_bytes, digest_b3,
    final_report_id_for_run, plan_id_for_run, run_key_for_ci, task_report_id_for_task,
    validate_final_report_id,
};

include!("impl_contract_reports/plan_matrix_task.rs");
include!("impl_contract_reports/final_workflow.rs");
include!("impl_contract_reports/matrix_identity_timing.rs");

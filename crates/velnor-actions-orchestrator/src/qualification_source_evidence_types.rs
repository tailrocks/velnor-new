//! Source-only Qualification evidence written before the producer run ends.

use serde::{Deserialize, Serialize};
use velnor_actions_contract::workflow::{
    MatrixStatus, PlanGenerator, QualificationCacheProducerContext, TaskStatus,
};

use crate::qualification_cache_runtime::closure::ObservedLaneClosure;

/// Source and report facts for one lane after task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QualificationSourceEvidenceDocument {
    /// Source evidence file format version.
    pub schema: u32,
    /// Actual workflow context captured by the producer runner.
    pub producer: QualificationCacheProducerContext,
    /// Source and report facts for one matrix lane after task execution.
    pub evidence: QualificationSourceEvidence,
}

/// Source and report facts for one plan entry with satisfactory results from the necessary checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QualificationSourceEvidence {
    /// Plan identity used to locate this lane.
    pub plan_id: String,
    /// Matrix identity from this job and the plan entry with satisfactory results from the necessary checks.
    pub matrix_key: String,
    /// Exact generated job identity.
    pub job_id: String,
    /// Exact task identity.
    pub task_id: String,
    /// Planned task identity in a different field from the source and report facts.
    pub planned_task_digest: String,
    /// Planned input identity in a different field from the source and report facts.
    pub planned_input_digest: String,
    /// Planned source-closure identity in a different field from the source and report facts.
    pub planned_closure_digest: String,
    /// Current source revision checked out by this runner.
    pub source_sha: String,
    /// Generator identity measured in the current producer process.
    pub generator: PlanGenerator,
    /// Task and input identities from the current source tree.
    pub closure: ObservedLaneClosure,
    /// Exact task report observed on disk.
    pub task_report: QualificationSourceReportObservation,
    /// Exact matrix report observed on disk.
    pub matrix_report: QualificationMatrixReportObservation,
    /// Cache evidence is added only after independent save and restore proof.
    pub cache_evidence_complete: bool,
}

/// Digest and outcome read from one task report in the JSON format given by the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QualificationSourceReportObservation {
    /// Task report identifier made by the contract.
    pub report_id: String,
    /// BLAKE3 digest of the report bytes in the JSON format given by the contract.
    pub digest_b3: String,
    /// Task outcome recorded by the report producer.
    pub status: TaskStatus,
    /// Process exit code recorded by the report producer.
    pub exit_code: i32,
}

/// Digest and outcome read from one matrix report in the JSON format given by the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QualificationMatrixReportObservation {
    /// Matrix report identifier made by the contract.
    pub report_id: String,
    /// BLAKE3 digest of the report bytes in the JSON format given by the contract.
    pub digest_b3: String,
    /// Aggregate outcome recorded by the report producer.
    pub status: MatrixStatus,
}

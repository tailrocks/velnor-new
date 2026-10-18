//! Source-only Qualification evidence written before the producer run ends.

use serde::{Deserialize, Serialize};
use velnor_actions_contract::workflow::{
    MatrixStatus, PlanGenerator, QualificationCacheProducerContext, TaskStatus,
};

use crate::qualification_cache_runtime::closure::ObservedLaneClosure;

/// Current per-lane source and report observations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QualificationSourceEvidenceDocument {
    /// Sidecar schema version.
    pub schema: u32,
    /// Actual workflow context captured by the producer runner.
    pub producer: QualificationCacheProducerContext,
    /// Source observations for one current matrix lane.
    pub evidence: QualificationSourceEvidence,
}

/// Source observations linked to one validated plan entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QualificationSourceEvidence {
    /// Plan identity used to locate this lane.
    pub plan_id: String,
    /// Matrix identity from the current job and validated plan.
    pub matrix_key: String,
    /// Exact generated job identity.
    pub job_id: String,
    /// Exact task identity.
    pub task_id: String,
    /// Planned task identity, retained separately from current observations.
    pub planned_task_digest: String,
    /// Planned input identity, retained separately from current observations.
    pub planned_input_digest: String,
    /// Planned source-closure identity, retained separately from observations.
    pub planned_closure_digest: String,
    /// Current source revision checked out by this runner.
    pub source_sha: String,
    /// Generator identity measured in the current producer process.
    pub generator: PlanGenerator,
    /// Task and input identities recomputed from the current source tree.
    pub closure: ObservedLaneClosure,
    /// Exact task report observed on disk.
    pub task_report: QualificationSourceReportObservation,
    /// Exact matrix report observed on disk.
    pub matrix_report: QualificationMatrixReportObservation,
    /// Cache evidence is added only after independent save and restore proof.
    pub cache_evidence_complete: bool,
}

/// Digest and outcome read from one canonical task report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QualificationSourceReportObservation {
    /// Contract-derived task report identifier.
    pub report_id: String,
    /// BLAKE3 digest of the canonical report bytes.
    pub digest_b3: String,
    /// Task outcome recorded by the report producer.
    pub status: TaskStatus,
    /// Process exit code recorded by the report producer.
    pub exit_code: i32,
}

/// Digest and outcome read from one canonical matrix report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QualificationMatrixReportObservation {
    /// Contract-derived matrix report identifier.
    pub report_id: String,
    /// BLAKE3 digest of the canonical report bytes.
    pub digest_b3: String,
    /// Aggregate outcome recorded by the report producer.
    pub status: MatrixStatus,
}

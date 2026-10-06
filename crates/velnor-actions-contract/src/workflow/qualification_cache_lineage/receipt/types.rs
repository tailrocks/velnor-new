//! Receipt and observation wire records.

use serde::{Deserialize, Serialize};

use super::super::identity::QualificationCacheLayer;
use super::super::runtime::QualificationRuntimeIdentity;
use crate::workflow::{QualificationPhase, QualificationRunRef};

/// Immutable uploaded receipt artifact name consumed by the dispatcher.
pub const QUALIFICATION_CACHE_RECEIPT_ARTIFACT: &str = "velnor-qualification-cache-receipt-v1";
/// The sole file inside each immutable qualification receipt artifact.
pub const QUALIFICATION_CACHE_RECEIPT_FILENAME: &str = "qualification-cache-receipt.json";
/// Maximum staged receipt document size before deserialization.
pub const MAX_QUALIFICATION_RECEIPT_BYTES: usize = 524_288;
/// Maximum phase-chain length including the current run.
pub const MAX_QUALIFICATION_RECEIPT_DEPTH: usize = 4;
/// Maximum JSON nesting accepted before deserialization.
pub(super) const MAX_JSON_NESTING: usize = 32;
/// Maximum UTF-8 bytes in one externally supplied text field.
pub(super) const MAX_RECEIPT_TEXT_BYTES: usize = 4_096;

/// Exact cache restore result observed in a completed producer job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheRestoreResult {
    /// Exact primary key was absent.
    Miss,
    /// A cache entry was restored.
    Hit,
    /// Cache access was disabled by the typed directive.
    Disabled,
}

/// Cache save action result, separate from backend creation evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheSaveActionResult {
    /// Save action reported successful completion.
    Succeeded,
    /// Save action reported that immutable content already existed or was skipped.
    Skipped,
    /// No save was needed because the exact useful-state digest was unchanged.
    NotRequired,
    /// Save action failed.
    Failed,
    /// Save was forbidden by the typed directive.
    Disabled,
}

/// One exact key/ref record returned by the cache backend API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheBackendEntry {
    /// Backend cache ID.
    pub id: u64,
    /// Full immutable cache key.
    pub key: String,
    /// GitHub cache ref scope.
    pub git_ref: String,
    /// Archive size in bytes.
    pub size_bytes: u64,
}

/// Result of one authoritative cache-list query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "entry", rename_all = "snake_case")]
pub enum QualificationCacheBackendObservation {
    /// API lookup completed and no matching exact key/ref existed.
    Absent,
    /// API lookup completed and returned this exact key/ref.
    Found(QualificationCacheBackendEntry),
    /// Layer access was disabled, so no API lookup was needed.
    NotQueried,
    /// API lookup failed or could not establish completeness.
    Unavailable,
}

/// Restore action outputs captured after the producer finishes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheRestore {
    /// Full key submitted to the restore action.
    pub requested_key: Option<String>,
    /// Full key reported as the matched archive, if any.
    pub matched_key: Option<String>,
    /// Backend object observed for the action's match; required before import.
    pub matched_cache: QualificationCacheBackendObservation,
    /// Observed restore result.
    pub result: QualificationCacheRestoreResult,
}

/// Save action plus before/after backend observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheSave {
    /// Full key submitted to the save action.
    pub requested_key: Option<String>,
    /// Action result; success alone does not prove archive creation.
    pub action: QualificationCacheSaveActionResult,
    /// Exact backend state before producer execution.
    pub before: QualificationCacheBackendObservation,
    /// Exact backend state after all producer post-job hooks completed.
    pub after: QualificationCacheBackendObservation,
}

/// One layer's actual restore and save observations for one matrix lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheLayerReceipt {
    /// Closed cache layer.
    pub layer: QualificationCacheLayer,
    /// Whether this layer is applicable to the selected lane.
    pub active: bool,
    /// Identity commitment for this lane and layer.
    pub identity_digest: String,
    /// Actual runtime image, ABI and toolchain evidence.
    pub runtime_identity: Option<QualificationRuntimeIdentity>,
    /// Actual digest of this layer's useful cache payload after the job.
    pub state_digest: Option<String>,
    /// Restore action output and exact matched key.
    pub restore: QualificationCacheRestore,
    /// Save action plus immutable backend creation evidence.
    pub save: QualificationCacheSave,
}

/// Per-lane completed work and cache observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheLaneReceipt {
    /// Stable plan matrix key.
    pub matrix_key: String,
    /// Stable stack and task identity.
    pub stack_id: String,
    /// Stable task-group identity.
    pub task_id: String,
    /// All obligation task IDs completed by this lane.
    pub completed_task_ids: Vec<String>,
    /// Actual post-build dependency/closure digest.
    pub closure_digest: String,
    /// Actual useful cache-state digest after job completion.
    pub useful_state_digest: String,
    /// Closed layer records, including explicit disabled `TaskResult` evidence.
    pub layers: Vec<QualificationCacheLayerReceipt>,
}

/// Link to the immutable receipt artifact admitted as a predecessor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheReceiptLink {
    /// Exact predecessor run and attempt.
    pub run: QualificationRunRef,
    /// Canonical BLAKE3 digest of the predecessor receipt JSON value.
    pub receipt_digest: String,
    /// Exact immutable artifact ID returned by GitHub.
    pub artifact_id: u64,
    /// Exact Actions API SHA-256 digest of the predecessor archive.
    pub artifact_digest: String,
}

/// Completed-run evidence; directives are not accepted as observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheReceipt {
    /// Receipt schema version.
    pub schema: u32,
    /// Derived plan identity for this exact run and attempt.
    pub plan_id: String,
    /// Completed producer run and attempt.
    pub run: QualificationRunRef,
    /// Isolated experiment namespace.
    pub campaign: String,
    /// Phase whose actual execution this receipt records.
    pub phase: QualificationPhase,
    /// Source revision checked out by this completed run.
    pub source_sha: String,
    /// Static configuration and complete obligation-set commitment.
    pub configuration_digest: String,
    /// Verified source delta, present only for `UsefulDelta` receipts.
    #[serde(default)]
    pub source_delta: Option<QualificationSourceDelta>,
    /// Immediate predecessor run and immutable receipt digest, when required.
    pub predecessor: Option<QualificationCacheReceiptLink>,
    /// Actual completed matrix lanes.
    pub lanes: Vec<QualificationCacheLaneReceipt>,
}

/// Bounded checked-out source change used only by `UsefulDelta`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationSourceDelta {
    /// Source SHA from the admitted Third run.
    pub base_source_sha: String,
    /// Source SHA checked out by the `UsefulDelta` run.
    pub source_sha: String,
    /// Sorted, unique paths returned by fixed-argv `git diff --name-only`.
    pub changed_paths: Vec<String>,
    /// BLAKE3 digest over the canonical changed path list.
    pub diff_digest: String,
    /// True only after the planner verified base is an ancestor of source.
    pub base_is_ancestor: bool,
}

/// Authoritative metadata returned for one workflow run and attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheRunMetadata {
    /// Canonical `owner/repository` returned by GitHub.
    pub repository: String,
    /// Repository default branch from the repository API.
    pub default_branch: String,
    /// Branch ref for this workflow run.
    pub git_ref: String,
    /// Whether the branch API marks this ref protected.
    pub ref_protected: bool,
    /// Workflow path/ref returned by the run API.
    pub workflow_path_ref: String,
    /// Exact runner-owned `GITHUB_WORKFLOW_REF` from the producer receipt.
    pub workflow_ref: String,
    /// Exact workflow SHA captured by the producer.
    pub workflow_sha: String,
    /// Head SHA returned by the run API.
    pub head_sha: String,
    /// Event name returned by the run API.
    pub event: String,
    /// Run conclusion returned after completion.
    pub conclusion: String,
    /// Exact run ID and attempt returned by the API.
    pub run: QualificationRunRef,
}

/// Immutable artifact metadata returned by the Actions artifact API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheArtifact {
    /// Artifact ID.
    pub id: u64,
    /// Artifact name.
    pub name: String,
    /// API SHA-256 digest in `sha256:<hex>` form.
    pub digest: String,
    /// Artifact size in bytes.
    pub size_bytes: u64,
    /// Whether GitHub marked the artifact expired.
    pub expired: bool,
    /// Run ID returned inside `workflow_run` artifact metadata.
    pub workflow_run_id: u64,
    /// Branch returned inside artifact workflow-run metadata.
    pub workflow_head_branch: String,
    /// Head SHA returned inside artifact workflow-run metadata.
    pub workflow_head_sha: String,
}

/// Runner-owned context captured by the completed producer workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheProducerContext {
    /// Repository slug observed by the runner.
    pub repository: String,
    /// Default branch observed during planning.
    pub default_branch: String,
    /// Exact `GITHUB_REF` captured by the runner.
    pub git_ref: String,
    /// Historical `GITHUB_REF_PROTECTED` value for the producer run.
    pub ref_protected: bool,
    /// Exact `GITHUB_WORKFLOW_REF` captured by the runner.
    pub workflow_ref: String,
    /// Exact `GITHUB_WORKFLOW_SHA` captured by the runner.
    pub workflow_sha: String,
    /// Exact `GITHUB_SHA` captured by the runner.
    pub source_sha: String,
    /// Exact `GITHUB_RUN_ID` and `GITHUB_RUN_ATTEMPT` values.
    pub run: QualificationRunRef,
}

/// One immutable receipt artifact file uploaded by the post-job collector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheReceiptArtifactDocument {
    /// Artifact document schema version.
    pub schema: u32,
    /// Runner context attested by the workflow itself.
    pub producer: QualificationCacheProducerContext,
    /// Completed job, action and backend observations.
    pub receipt: QualificationCacheReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdmissionNode {
    pub(super) metadata: QualificationCacheRunMetadata,
    pub(super) artifact: QualificationCacheArtifact,
    pub(super) producer: QualificationCacheProducerContext,
    pub(super) receipt: QualificationCacheReceipt,
    #[serde(default)]
    pub(super) previous: Option<Box<AdmissionNode>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdmissionDocument {
    pub(super) predecessor: AdmissionNode,
    #[serde(default)]
    pub(super) source_delta: Option<QualificationSourceDelta>,
}

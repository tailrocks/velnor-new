//! Merge-owned vocabulary shared with cover: request and manifests.

use serde::{Deserialize, Serialize};
use velnor_actions_contract_workflow::{
    ArtifactBuildObservation, ArtifactBuildRunContext, MatrixReport, Plan, PlanMatrix,
    RequiredJobResult, TaskReport, WorkflowEvent,
};

use super::shard_types::{ResourceLimits, ShardProof};

/// `merge-v1` request: plan, matrix bytes, reports, and jobs.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeRequest {
    /// Request schema; must be 1.
    pub schema: u32,
    /// Run key.
    pub run_key: String,
    /// Merge-time triggering event captured at assembly; the plan's
    /// stamped event must match it exactly (a forged plan claiming a
    /// stronger event fails closed instead of inheriting its stamp).
    #[serde(default)]
    pub actual_event: Option<WorkflowEvent>,
    /// Head-bound candidate attestation; required in candidate mode.
    #[serde(default)]
    pub candidate_attestation: Option<CandidateAttestation>,
    /// GitHub-owned identity for the current artifact-build run attempt.
    #[serde(default)]
    pub artifact_build_context: Option<ArtifactBuildRunContext>,
    /// Attempt-scoped Actions API and checksum observations for planned outputs.
    #[serde(default)]
    pub artifact_build_observations: Vec<ArtifactBuildObservation>,
    /// Validated plan; absent when the plan artifact never landed.
    #[serde(default)]
    pub plan: Option<Plan>,
    /// `matrix.json` content; must agree with the plan matrix.
    #[serde(default)]
    pub matrix: Option<PlanMatrix>,
    /// Matrix reports to aggregate.
    pub matrix_reports: Vec<MatrixReport>,
    /// Per-task report files backing every aggregate entry.
    #[serde(default)]
    pub task_reports: Vec<TaskReport>,
    /// Named-check receipt plus exact downloaded scenario bytes.
    #[serde(default)]
    pub check_proofs: Vec<serde_json::Value>,
    /// Declared validator inventory from the workflow `needs` channel.
    pub required_job_ids: Vec<String>,
    /// Observed validator conclusions covering the inventory exactly.
    pub required_jobs: Vec<RequiredJobResult>,
    /// Assembly failure details; every entry fails the verdict.
    #[serde(default)]
    pub assembly_errors: Vec<String>,
    /// Trusted baseline manifest for coverage revalidation.
    #[serde(default)]
    pub baseline_manifest: Option<BaselineManifest>,
    /// Shard proofs for partitioned test entries.
    #[serde(default)]
    pub shard_proofs: Vec<ShardProof>,
    /// Configured resource limits revalidated here.
    #[serde(default)]
    pub limits: Option<ResourceLimits>,
    /// Sequential-reference obligation set.
    #[serde(default)]
    pub reference_task_ids: Option<Vec<String>>,
}

/// Head-bound candidate attestation written by the candidate job.
///
/// The candidate job observes the plan head from its downloaded plan
/// artifact and embeds it as `commit`; the merge re-checks equality
/// against its own plan head, so a stale or cross-plan candidate
/// artifact fails closed instead of qualifying the wrong commit.
/// Tokens reuse the closed miss set: absent is `source_missing`,
/// mismatched is `trust_scope_mismatch`.
#[derive(Debug, serde::Deserialize)]
pub struct CandidateAttestation {
    /// Attestation schema; must be 1.
    pub schema: u32,
    /// Plan head observed by the candidate job.
    pub commit: String,
}

/// One trusted-baseline task proof: identities plus provenance run IDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineTaskEntry {
    /// Covered task ID.
    pub task_id: String,
    /// Covered task digest.
    pub task_digest: String,
    /// Covered input digest.
    pub input_digest: String,
    /// Canonical digest over the task's complete input closure.
    ///
    /// Required with no default: entries recorded before closure binding
    /// (schema 1) fail deserialization instead of covering blindly.
    pub closure_digest: String,
    /// Original direct-execution proof run.
    pub proof_run_id: u64,
    /// Carrying run that revalidated the proof.
    pub observed_run_id: u64,
    /// External-data freshness (required for advisory kinds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_data:
        Option<velnor_actions_orchestrator_external_data::external_data::ExternalDataFreshness>,
    /// Structured task proof, when the publisher recorded one (PAR-5.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<velnor_actions_contract_workflow::ManifestTaskProof>,
}

/// Trusted `baseline.json`: minimum shape plus artifact binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineManifest {
    /// Manifest schema; must be 2 (closure-bound entries).
    pub schema: u32,
    /// Repository identity digest.
    pub repository_id: String,
    /// Exact trusted source commit.
    pub source_commit: String,
    /// Protected ref under test.
    #[serde(rename = "ref")]
    pub ref_: String,
    /// Protected event; must be `push`.
    pub event: String,
    /// Protected workflow ref.
    pub workflow_ref: String,
    /// Proof run ID.
    pub run_id: u64,
    /// Proof run attempt.
    pub run_attempt: u64,
    /// Final result; must be `passed`.
    pub final_status: String,
    /// Generator version.
    pub generator_version: String,
    /// Generator SHA-256.
    pub generator_sha256: String,
    /// Compatibility identity.
    pub compatibility_id: String,
    /// Manifest-assigned numeric fingerprint of the artifact name.
    pub artifact_id: u64,
    /// Derived baseline artifact name.
    pub artifact_name: String,
    /// Per-task proofs.
    pub tasks: Vec<BaselineTaskEntry>,
    /// Unix expiry; absent means the baseline never expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_unix: Option<u64>,
}

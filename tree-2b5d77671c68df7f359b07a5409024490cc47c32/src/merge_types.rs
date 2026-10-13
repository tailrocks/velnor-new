//! Merge-owned vocabulary shared with cover: request and manifests.

use std::collections::BTreeSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use velnor_actions_contract_workflow::{
    ArtifactBuildObservation, ArtifactBuildRunContext, JobConclusion, MatrixReport, Plan,
    PlanMatrix, RequiredJobResult, TaskReport, WorkflowEvent,
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
    /// Optional raw report-artifact/check-run output fan-in.
    ///
    /// Absence preserves the ordinary schema-1 request shape. Presence is
    /// parsed strictly but does not itself establish provider comparison.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present_task_report_outputs"
    )]
    pub task_report_outputs: Option<TaskReportOutputFanIn>,
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

/// Deserialize a present sidecar as a value so explicit `null` is malformed.
fn deserialize_present_task_report_outputs<'de, D>(
    deserializer: D,
) -> Result<Option<TaskReportOutputFanIn>, D::Error>
where
    D: Deserializer<'de>,
{
    TaskReportOutputFanIn::deserialize(deserializer).map(Some)
}

/// Optional public-GitHub report-output fan-in carried beside ordinary
/// schema-1 evidence. Its producer list is self-contained metadata; callers
/// must bind its claimed producer census to the actual producer graph before
/// using it for any provider comparison.
#[derive(Debug, Clone, Serialize)]
pub struct TaskReportOutputFanIn {
    /// Sidecar schema version.
    pub schema: u32,
    /// Supported provider origin. GHES is not represented by this type.
    pub origin: TaskReportOutputOrigin,
    /// GitHub repository and workflow-run identity.
    pub run: ArtifactBuildRunContext,
    /// Source SHA recorded by the plan job.
    pub head_sha: String,
    /// Canonical digest recorded for the plan.
    pub plan_digest: String,
    /// Claimed producer-key census against which `producers` is validated.
    /// This value is not trusted graph input until independently bound.
    pub expected_workflow_job_keys: Vec<String>,
    /// Raw output values for the expected successful producer jobs.
    pub producers: Vec<TaskReportProducerOutput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskReportOutputFanInWire {
    schema: u32,
    origin: TaskReportOutputOrigin,
    run: ArtifactBuildRunContext,
    head_sha: String,
    plan_digest: String,
    expected_workflow_job_keys: Vec<String>,
    producers: Vec<TaskReportProducerOutput>,
}

impl<'de> Deserialize<'de> for TaskReportOutputFanIn {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = TaskReportOutputFanInWire::deserialize(deserializer)?;
        let value = Self {
            schema: wire.schema,
            origin: wire.origin,
            run: wire.run,
            head_sha: wire.head_sha,
            plan_digest: wire.plan_digest,
            expected_workflow_job_keys: wire.expected_workflow_job_keys,
            producers: wire.producers,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

impl TaskReportOutputFanIn {
    /// Parse one sidecar and validate its internal structure through merge.
    ///
    /// The claimed producer-key list is checked against the records, never
    /// inferred from `Plan.matrix.include`. This does not establish graph
    /// completeness or provider comparison.
    ///
    /// # Errors
    ///
    /// Returns a serde error when the sidecar is malformed or internally
    /// inconsistent.
    pub fn parse_value(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value)
    }

    fn validate(&self) -> Result<(), &'static str> {
        if self.schema != 1
            || self.expected_workflow_job_keys.is_empty()
            || self.producers.is_empty()
        {
            return Err("invalid_task_report_outputs");
        }

        let mut expected = BTreeSet::new();
        for key in &self.expected_workflow_job_keys {
            velnor_actions_contract::ids::job_ids::validate_job_id(key)
                .map_err(|_| "invalid_task_report_outputs")?;
            if !expected.insert(key.as_str()) {
                return Err("invalid_task_report_outputs");
            }
        }

        let mut observed = BTreeSet::new();
        for producer in &self.producers {
            velnor_actions_contract::ids::job_ids::validate_job_id(&producer.workflow_job_key)
                .map_err(|_| "invalid_task_report_outputs")?;
            if producer.conclusion != JobConclusion::Success
                || !expected.contains(producer.workflow_job_key.as_str())
                || !observed.insert(producer.workflow_job_key.as_str())
            {
                return Err("invalid_task_report_outputs");
            }
        }
        if observed != expected {
            return Err("invalid_task_report_outputs");
        }
        Ok(())
    }
}

/// Fixed origin for the supported public GitHub.com API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskReportOutputOrigin {
    /// GitHub.com Actions API.
    GithubCom,
}

/// Report output values emitted by one logical workflow job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReportProducerOutput {
    /// Workflow job key, distinct from the REST workflow-job ID.
    pub workflow_job_key: String,
    /// Job conclusion; only success is accepted by this sidecar parser.
    pub conclusion: JobConclusion,
    /// Upload-artifact ID namespace.
    pub artifact_id: TaskReportArtifactId,
    /// Check-run ID namespace; never aliases the REST workflow-job ID.
    pub check_run_id: TaskReportCheckRunId,
}

/// Positive numeric upload-artifact ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskReportArtifactId(i64);

impl TaskReportArtifactId {
    /// Return the numeric upload-artifact ID.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Serialize for TaskReportArtifactId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i64(self.0)
    }
}

impl<'de> Deserialize<'de> for TaskReportArtifactId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(D::Error::custom("invalid_task_report_outputs"))
        }
    }
}

/// Positive numeric GitHub Check Run ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskReportCheckRunId(i64);

impl TaskReportCheckRunId {
    /// Return the numeric Check Run ID.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Serialize for TaskReportCheckRunId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i64(self.0)
    }
}

impl<'de> Deserialize<'de> for TaskReportCheckRunId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(D::Error::custom("invalid_task_report_outputs"))
        }
    }
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

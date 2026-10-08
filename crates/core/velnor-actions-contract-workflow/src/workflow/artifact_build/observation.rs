//! Serialized Actions API and downloaded-output evidence for one artifact lane.

use serde::{Deserialize, Serialize};

use crate::workflow::JobConclusion;

use super::{ArtifactBuildIdentity, ArtifactBuildResult};

/// One API job and artifact observation for a planned provider/task pair.
///
/// Optional API fields preserve missing or incomplete GitHub evidence as data;
/// the reconciler rejects it instead of omitting a required task silently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBuildObservation {
    /// Provider/task identity derived from the authoritative plan.
    pub identity: ArtifactBuildIdentity,
    /// Run ID from the attempt-scoped workflow jobs endpoint.
    pub api_run_id: String,
    /// Source SHA from the attempt-scoped workflow jobs endpoint.
    pub api_head_sha: String,
    /// Numeric Actions API workflow job ID, absent when no job was observed.
    pub api_job_id: Option<u64>,
    /// Explicit job display name returned by Actions API.
    pub api_job_name: String,
    /// API job status; only `completed` can reconcile successfully.
    pub api_job_status: String,
    /// Actual Actions runner ID, when assigned.
    pub api_runner_id: Option<u64>,
    /// Actual Actions runner name, when assigned.
    pub api_runner_name: Option<String>,
    /// Actual Actions runner group ID, when assigned.
    pub api_runner_group_id: Option<u64>,
    /// Actual Actions runner group name, when assigned.
    pub api_runner_group_name: Option<String>,
    /// Runner labels returned by the workflow jobs endpoint.
    pub api_runner_labels: Vec<String>,
    /// Numeric Actions artifact ID, absent when no artifact was observed.
    pub api_artifact_id: Option<u64>,
    /// Artifact name returned by Actions API.
    pub api_artifact_name: Option<String>,
    /// Compressed artifact size returned by Actions API.
    pub api_artifact_size_bytes: Option<u64>,
    /// Expiry state returned by Actions API.
    pub api_artifact_expired: Option<bool>,
    /// Run ID associated with the artifact by Actions API.
    pub api_artifact_run_id: Option<String>,
    /// Repository ID associated with the artifact by Actions API.
    pub api_artifact_repository_id: Option<String>,
    /// Source SHA associated with the artifact by Actions API.
    pub api_artifact_head_sha: Option<String>,
    /// GitHub's actual job conclusion; missing and non-success are not passes.
    pub conclusion: JobConclusion,
    /// Downloaded result manifest; absent is never success.
    pub result: Option<ArtifactBuildResult>,
    /// Bounded digests of downloaded files, computed by the trusted retriever.
    pub downloaded_outputs: Vec<DownloadedArtifactOutput>,
}

/// One declared output reread from an Actions artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadedArtifactOutput {
    /// Declared output ID associated by the result manifest.
    pub output_id: String,
    /// Exact repository-relative path.
    pub path: String,
    /// Raw file length from the downloaded file handle.
    pub size_bytes: u64,
    /// BLAKE3 digest computed from the downloaded file handle.
    pub digest: String,
}

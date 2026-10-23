//! Versioned data-only envelope for verified local GitHub evidence inputs.

use serde::{Deserialize, Serialize};

#[path = "github_evidence_snapshot_spec_validation.rs"]
mod spec_validation;
#[path = "github_evidence_snapshot_validation.rs"]
mod validation;
pub use validation::{is_valid_github_evidence_conclusion, is_valid_github_evidence_status};

/// Current independently versioned evidence snapshot schema.
pub const GITHUB_EVIDENCE_SNAPSHOT_SCHEMA: u32 = 1;
/// Maximum indexed metadata responses in one snapshot.
pub const MAX_GITHUB_EVIDENCE_DOCUMENTS: usize = 1_024;
/// Maximum captured attempts in one snapshot.
pub const MAX_GITHUB_EVIDENCE_ATTEMPTS: usize = 2_000;
/// Maximum original artifact archives in one snapshot.
pub const MAX_GITHUB_EVIDENCE_ARCHIVES: usize = 4_096;
/// Maximum size of one metadata response body.
pub const MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum size of one original artifact ZIP.
pub const MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum total bytes in a snapshot bundle, including the index.
pub const MAX_GITHUB_EVIDENCE_BUNDLE_BYTES: u64 = 1024 * 1024 * 1024;

/// Canonical generator-resolved inputs passed only to the trusted collector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceFetchSpecV1 {
    /// Closed schema version.
    pub schema: u32,
    /// Exact schedule-only collector suite ID.
    pub collector_suite_id: String,
    /// Exact generated collector workflow path.
    pub collector_workflow_path: String,
    /// Repository default branch selected during generation.
    pub default_branch: String,
    /// Bounded number of days before the scheduled collector run.
    pub window_days: u8,
    /// Generated workflow identities to query.
    pub workflow_sources: Vec<GitHubEvidenceWorkflowRefV1>,
    /// Exact generated task-suite artifact declarations to resolve.
    pub artifact_sources: Vec<GitHubEvidenceArtifactRefV1>,
}

/// One workflow identity resolved from a closed configuration reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceWorkflowRefV1 {
    /// Closed generated workflow identity.
    pub source: GitHubEvidenceWorkflowSourceRefV1,
    /// Exact generated workflow path.
    pub path: String,
    /// Exact suite job ID; absent for primary CI metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    /// Stable generated display name returned by the Actions jobs API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_name: Option<String>,
}

/// Workflow identity carried into the snapshot envelope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitHubEvidenceWorkflowSourceRefV1 {
    /// Generator-owned `.github/workflows/ci.yml`.
    PrimaryCi,
    /// Generated companion workflow for one exact task suite.
    TaskSuite {
        /// Exact suite ID.
        suite_id: String,
    },
}

/// One artifact resolved from a declared suite output.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceArtifactRefV1 {
    /// Exact suite ID.
    pub suite_id: String,
    /// Exact configured artifact declaration name.
    pub artifact_name: String,
    /// Exact generator-owned workflow path.
    pub workflow_path: String,
    /// Exact generated suite producer job ID.
    pub job_id: String,
    /// Stable generated display name returned by the Actions jobs API.
    pub job_name: String,
}

/// API document type captured in the bundle; routes are constructed internally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubEvidenceDocumentKindV1 {
    /// Repository ID, canonical name and default branch.
    Repository,
    /// One page of generated workflow inventory.
    WorkflowInventory,
    /// One page of the bounded workflow-run query.
    WorkflowRunsPage,
    /// The latest run before the bounded workflow-run window.
    WorkflowPredecessor,
    /// One exact run attempt detail record.
    RunAttempt,
    /// One page of jobs for an exact run attempt.
    RunAttemptJobsPage,
    /// One page of artifact metadata for an exact run.
    RunArtifactsPage,
    /// One metadata record re-read through the exact artifact-ID endpoint.
    ArtifactById,
}

/// Digest and request identity of a captured metadata response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceDocumentV1 {
    /// Typed response kind.
    pub kind: GitHubEvidenceDocumentKindV1,
    /// Exact workflow path if the endpoint is workflow-scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_path: Option<String>,
    /// Exact run ID if the endpoint is run-scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,
    /// Exact run attempt if the endpoint is attempt-scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    /// Exact artifact ID if the endpoint is artifact-scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<u64>,
    /// Pagination page, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    /// Opaque generated path below the bundle root.
    pub path: String,
    /// Exact response length in bytes.
    pub size_bytes: u64,
    /// Lowercase SHA-256 of the unmodified response body.
    pub sha256: String,
}

/// One complete workflow-run attempt captured from the Actions API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceRunAttemptV1 {
    /// Exact generated workflow path.
    pub workflow_path: String,
    /// Immutable GitHub workflow run identity.
    pub run_id: u64,
    /// Attempt number; first attempt is `1`.
    pub attempt: u32,
    /// Original workflow-run creation time in strict UTC RFC 3339 form.
    pub created_at: String,
    /// True only for the immediately preceding run before the lookback window.
    pub is_predecessor: bool,
    /// Event name returned by GitHub.
    pub event: String,
    /// Branch name returned by GitHub.
    pub head_branch: String,
    /// Full source commit SHA.
    pub head_sha: String,
    /// GitHub attempt status.
    pub status: String,
    /// GitHub attempt conclusion, if completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<String>,
    /// Job document indices covering this attempt's complete job pagination.
    pub jobs_documents: Vec<u32>,
}

/// Original archive bytes for one exact artifact ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceArtifactCaptureV1 {
    /// Declared producer identity.
    pub source: GitHubEvidenceArtifactRefV1,
    /// Immutable Actions artifact ID.
    pub artifact_id: u64,
    /// Exact source run and attempt.
    pub run_id: u64,
    /// Exact attempt-qualified artifact name returned by GitHub.
    pub name: String,
    /// Opaque generated path below the bundle root.
    pub path: String,
    /// Index of the exact by-ID metadata document for this archive.
    pub metadata_document: u32,
    /// Exact API metadata size.
    pub size_bytes: u64,
    /// Exact API metadata digest (SHA-256).
    pub sha256: String,
}

/// Repository identity captured at acquisition time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceRepositoryV1 {
    /// Immutable repository numeric ID.
    pub id: u64,
    /// Exact owner returned by GitHub.
    pub owner: String,
    /// Exact repository name returned by GitHub.
    pub name: String,
    /// Default branch bound by generated configuration.
    pub default_branch: String,
}

/// Same-run acquisition context required by the consumer to reject stale bundles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceAcquisitionV1 {
    /// Current scheduled workflow run ID.
    pub run_id: u64,
    /// Current scheduled workflow attempt.
    pub run_attempt: u32,
    /// Exact scheduled default-branch ref from the triggering event.
    pub collector_ref: String,
    /// Exact scheduled event SHA checked out by the task consumer.
    pub collector_sha: String,
    /// UTC timestamp when acquisition began.
    pub captured_at: String,
    /// Inclusive beginning of the requested lookback window.
    pub window_start: String,
    /// Exclusive end of the requested lookback window.
    pub window_end: String,
}

/// Complete verified index for one original-byte evidence bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceSnapshotV1 {
    /// Closed independently versioned schema.
    pub schema: u32,
    /// SHA-256 of canonical fetch specification bytes.
    pub spec_sha256: String,
    /// Immutable repository and default-branch identity.
    pub repository: GitHubEvidenceRepositoryV1,
    /// Same-run acquisition identity and temporal window.
    pub acquisition: GitHubEvidenceAcquisitionV1,
    /// Exact selected generated workflow identities.
    pub workflow_sources: Vec<GitHubEvidenceWorkflowRefV1>,
    /// Every captured API response, including all required pages and attempts.
    pub documents: Vec<GitHubEvidenceDocumentV1>,
    /// Every workflow attempt covered by the source query.
    pub run_attempts: Vec<GitHubEvidenceRunAttemptV1>,
    /// Exact declared producer artifacts and original ZIP bytes.
    pub artifacts: Vec<GitHubEvidenceArtifactCaptureV1>,
}

impl GitHubEvidenceSnapshotV1 {
    /// Validate the complete closed snapshot against its generated request.
    /// # Errors
    pub fn validate_for_spec(&self, spec: &GitHubEvidenceFetchSpecV1) -> Result<(), String> {
        spec.validate()?;
        let spec_bytes = crate::canonical_json_bytes(spec)
            .map_err(|_| "github_evidence_spec_canonicalization".to_owned())?;
        if self.schema != GITHUB_EVIDENCE_SNAPSHOT_SCHEMA
            || self.spec_sha256 != crate::canonical::sha256_hex(&spec_bytes)
            || self.workflow_sources != spec.workflow_sources
            || self.repository.default_branch != spec.default_branch
            || self.repository.id == 0
            || !crate::config::is_valid_github_owner(&self.repository.owner)
            || !crate::config::is_valid_github_repo_name(&self.repository.name)
            || self.run_attempts.is_empty()
            || self.run_attempts.len() > MAX_GITHUB_EVIDENCE_ATTEMPTS
            || self.documents.is_empty()
            || self.documents.len() > MAX_GITHUB_EVIDENCE_DOCUMENTS
            || self.artifacts.len() > MAX_GITHUB_EVIDENCE_ARCHIVES
            || !validation::is_utc_timestamp(&self.acquisition.captured_at)
            || !validation::is_utc_timestamp(&self.acquisition.window_start)
            || !validation::is_utc_timestamp(&self.acquisition.window_end)
            || !validation::valid_window(&self.acquisition, spec.window_days)
            || self.acquisition.collector_ref != format!("refs/heads/{}", spec.default_branch)
            || !crate::ids::is_lower_hex_len(&self.acquisition.collector_sha, 40)
            || self.acquisition.window_start >= self.acquisition.window_end
            || self.acquisition.run_id == 0
            || self.acquisition.run_attempt == 0
        {
            return Err("github_evidence_snapshot_header".to_owned());
        }
        if self
            .documents
            .iter()
            .filter(|document| document.kind == GitHubEvidenceDocumentKindV1::Repository)
            .count()
            != 1
        {
            return Err("github_evidence_repository_document_count".to_owned());
        }
        let mut total_bytes = 0_u64;
        let mut paths = std::collections::BTreeSet::<String>::new();
        for document in &self.documents {
            if !document.path.starts_with("documents/")
                || !validation::valid_bundle_path(&document.path)
                || !paths.insert(document.path.clone())
                || document.size_bytes == 0
                || document.size_bytes > MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES
                || !crate::ids::is_lower_hex_len(&document.sha256, 64)
                || !validation::valid_document_identity(document)
            {
                return Err("github_evidence_snapshot_document".to_owned());
            }
            total_bytes = total_bytes.saturating_add(document.size_bytes);
        }
        validation::validate_document_pages(&self.documents, spec)?;
        validation::validate_attempts(self, spec)?;
        validation::validate_artifacts(self, spec, &mut paths, &mut total_bytes)?;
        if total_bytes > MAX_GITHUB_EVIDENCE_BUNDLE_BYTES {
            return Err("github_evidence_snapshot_bundle_too_large".to_owned());
        }
        Ok(())
    }

    /// Exact safe file set expected below the uploaded bundle directory.
    #[must_use]
    pub fn expected_bundle_files(&self) -> std::collections::BTreeSet<String> {
        let mut files = self
            .documents
            .iter()
            .map(|document| document.path.clone())
            .collect::<std::collections::BTreeSet<_>>();
        files.extend(self.artifacts.iter().map(|artifact| artifact.path.clone()));
        files.insert("index.json".to_owned());
        files
    }
}

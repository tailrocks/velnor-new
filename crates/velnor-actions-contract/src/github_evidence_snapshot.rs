// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

#[path = "github_evidence_snapshot_spec_validation.rs"]
mod spec_validation;
#[path = "github_evidence_snapshot_validation.rs"]
mod validation;
pub use validation::{is_valid_github_evidence_conclusion, is_valid_github_evidence_status};

pub const GITHUB_EVIDENCE_SNAPSHOT_SCHEMA: u32 = 1;

pub const MAX_GITHUB_EVIDENCE_DOCUMENTS: usize = 1_024;

pub const MAX_GITHUB_EVIDENCE_ATTEMPTS: usize = 2_000;

pub const MAX_GITHUB_EVIDENCE_ARCHIVES: usize = 4_096;

pub const MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;

pub const MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;

pub const MAX_GITHUB_EVIDENCE_BUNDLE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceFetchSpecV1 {
    pub schema: u32,

    pub collector_suite_id: String,

    pub collector_workflow_path: String,

    pub default_branch: String,

    pub window_days: u8,

    pub workflow_sources: Vec<GitHubEvidenceWorkflowRefV1>,

    pub artifact_sources: Vec<GitHubEvidenceArtifactRefV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceWorkflowRefV1 {
    pub source: GitHubEvidenceWorkflowSourceRefV1,

    pub path: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitHubEvidenceWorkflowSourceRefV1 {
    PrimaryCi,

    TaskSuite { suite_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceArtifactRefV1 {
    pub suite_id: String,

    pub artifact_name: String,

    pub workflow_path: String,

    pub job_id: String,

    pub job_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubEvidenceDocumentKindV1 {
    Repository,

    WorkflowInventory,

    WorkflowRunsPage,

    WorkflowPredecessor,

    RunAttempt,

    RunAttemptJobsPage,

    RunArtifactsPage,

    ArtifactById,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceDocumentV1 {
    pub kind: GitHubEvidenceDocumentKindV1,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_path: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,

    pub path: String,

    pub size_bytes: u64,

    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceRunAttemptV1 {
    pub workflow_path: String,

    pub run_id: u64,

    pub attempt: u32,

    pub created_at: String,

    pub is_predecessor: bool,

    pub event: String,

    pub head_branch: String,

    pub head_sha: String,

    pub status: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<String>,

    pub jobs_documents: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceArtifactCaptureV1 {
    pub source: GitHubEvidenceArtifactRefV1,

    pub artifact_id: u64,

    pub run_id: u64,

    pub name: String,

    pub path: String,

    pub metadata_document: u32,

    pub size_bytes: u64,

    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceRepositoryV1 {
    pub id: u64,

    pub owner: String,

    pub name: String,

    pub default_branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceAcquisitionV1 {
    pub run_id: u64,

    pub run_attempt: u32,

    pub collector_ref: String,

    pub collector_sha: String,

    pub captured_at: String,

    pub window_start: String,

    pub window_end: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceSnapshotV1 {
    pub schema: u32,

    pub spec_sha256: String,

    pub repository: GitHubEvidenceRepositoryV1,

    pub acquisition: GitHubEvidenceAcquisitionV1,

    pub workflow_sources: Vec<GitHubEvidenceWorkflowRefV1>,

    pub documents: Vec<GitHubEvidenceDocumentV1>,

    pub run_attempts: Vec<GitHubEvidenceRunAttemptV1>,

    pub artifacts: Vec<GitHubEvidenceArtifactCaptureV1>,
}

impl GitHubEvidenceSnapshotV1 {
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

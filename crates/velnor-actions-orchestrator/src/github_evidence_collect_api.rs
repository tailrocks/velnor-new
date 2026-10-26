// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use serde::de::DeserializeOwned;
use velnor_actions_contract::MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES;
use velnor_actions_contract::strict_json::parse_strict_json_bytes;

use crate::OrchestratorError;
use crate::internal::internal;

pub(super) type EvidenceResult<T> = Result<T, OrchestratorError>;

#[derive(Debug, Deserialize)]
pub(super) struct RepositoryResponse {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) full_name: String,
    pub(super) default_branch: String,
    pub(super) owner: OwnerResponse,
}

#[derive(Debug, Deserialize)]
pub(super) struct OwnerResponse {
    pub(super) login: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowInventoryPage {
    pub(super) total_count: u64,
    pub(super) workflows: Vec<WorkflowResponse>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowResponse {
    pub(super) id: u64,
    pub(super) path: String,
    pub(super) state: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowRunsPage {
    pub(super) total_count: u64,
    pub(super) workflow_runs: Vec<WorkflowRunResponse>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowAttemptDetail {
    pub(super) id: u64,
    pub(super) workflow_id: u64,
    pub(super) run_attempt: u32,
    pub(super) event: String,
    pub(super) head_branch: String,
    pub(super) head_sha: String,
    pub(super) status: String,
    pub(super) conclusion: Option<String>,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct WorkflowRunResponse {
    pub(super) id: u64,
    pub(super) workflow_id: u64,
    pub(super) run_attempt: u32,
    pub(super) event: String,
    pub(super) head_branch: String,
    pub(super) head_sha: String,
    pub(super) status: String,
    pub(super) conclusion: Option<String>,
    pub(super) created_at: String,
    pub(super) repository: RepositoryIdentity,
    pub(super) head_repository: Option<RepositoryIdentity>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RepositoryIdentity {
    pub(super) id: u64,
    pub(super) full_name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct JobsPage {
    pub(super) total_count: u64,
    pub(super) jobs: Vec<JobResponse>,
}

#[derive(Debug, Deserialize)]
pub(super) struct JobResponse {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) run_id: u64,
    pub(super) head_sha: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ArtifactsPage {
    pub(super) total_count: u64,
    pub(super) artifacts: Vec<ArtifactResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct ArtifactResponse {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) size_in_bytes: u64,
    pub(super) expired: bool,
    pub(super) digest: Option<String>,
    pub(super) workflow_run: Option<ArtifactWorkflowRun>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct ArtifactWorkflowRun {
    pub(super) id: u64,
    pub(super) head_sha: String,
    pub(super) head_branch: String,
    pub(super) repository_id: u64,
    pub(super) head_repository_id: u64,
}

pub(super) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> EvidenceResult<T> {
    let limit = usize::try_from(MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES)
        .map_err(|_| internal("github_evidence_document_bound"))?;
    let value = parse_strict_json_bytes(bytes, limit)
        .map_err(|_| internal("github_evidence_json_invalid"))?;
    serde_json::from_value(value).map_err(|_| internal("github_evidence_response_shape"))
}

pub(super) fn expected_pages(total: u64, per_page: u64) -> EvidenceResult<u32> {
    if per_page == 0 {
        return Err(internal("github_evidence_page_size"));
    }
    let pages = total.div_ceil(per_page).max(1);
    u32::try_from(pages).map_err(|_| internal("github_evidence_page_count"))
}

pub(super) fn expected_page_items(total: u64, per_page: u64, page: u32) -> EvidenceResult<usize> {
    let offset = u64::from(page.saturating_sub(1)).saturating_mul(per_page);
    let count = total.saturating_sub(offset).min(per_page);
    usize::try_from(count).map_err(|_| internal("github_evidence_page_count"))
}

pub(super) fn artifact_digest(value: Option<&str>) -> Option<String> {
    let digest = value?.strip_prefix("sha256:")?;
    (digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then(|| digest.to_owned())
}

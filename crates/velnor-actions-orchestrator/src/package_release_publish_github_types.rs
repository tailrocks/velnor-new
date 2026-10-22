//! Strictly typed responses used by the fixed GitHub API adapter.

use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct RepositoryResponse {
    pub id: u64,
    pub full_name: Option<String>,
}
#[derive(Deserialize)]
pub(crate) struct ImmutableResponse {
    pub enabled: Option<bool>,
}
#[derive(Deserialize)]
pub(crate) struct GitRefResponse {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub object: GitObject,
}
#[derive(Deserialize)]
pub(crate) struct GitObject {
    pub sha: String,
    #[serde(rename = "type")]
    pub object_type: String,
}
#[derive(Deserialize)]
pub(crate) struct GitTagResponse {
    pub sha: String,
    pub tag: String,
    pub message: String,
    pub object: GitObject,
}
#[derive(Deserialize)]
pub(crate) struct TagObjectResponse {
    pub sha: String,
    pub tag: String,
    pub message: String,
    pub object: GitObject,
}
pub(crate) struct TagObservation {
    pub source_sha: String,
    pub message: Option<String>,
}
#[derive(Clone, Deserialize)]
pub(crate) struct WorkflowRunResponse {
    pub id: Option<u64>,
    pub run_attempt: Option<u32>,
    pub run_number: Option<u64>,
    pub workflow_id: Option<u64>,
    pub name: Option<String>,
    pub path: Option<String>,
    pub head_sha: Option<String>,
    pub head_branch: Option<String>,
    pub event: Option<String>,
    pub status: Option<String>,
    pub conclusion: Option<String>,
    pub repository: Option<RunRepository>,
    pub head_repository: Option<RunRepository>,
}
#[derive(Clone, Deserialize)]
pub(crate) struct RunRepository {
    pub id: u64,
}

#[derive(Deserialize)]
pub(crate) struct WorkflowMetadata {
    pub id: u64,
    pub name: String,
    pub path: String,
    pub state: String,
}

#[derive(Deserialize)]
pub(crate) struct WorkflowRunList {
    pub workflow_runs: Vec<WorkflowRunResponse>,
}
#[derive(Deserialize)]
pub(crate) struct WorkflowJobList {
    pub jobs: Vec<WorkflowJob>,
}
#[derive(Clone, Deserialize)]
pub(crate) struct WorkflowJob {
    pub run_id: u64,
    pub name: String,
    pub head_sha: Option<String>,
    pub status: Option<String>,
    pub conclusion: Option<String>,
}
#[derive(Deserialize)]
pub(crate) struct WorkflowArtifactList {
    pub total_count: usize,
    pub artifacts: Vec<WorkflowArtifact>,
}
#[derive(Deserialize)]
pub(crate) struct WorkflowArtifact {
    pub id: u64,
    pub name: String,
    pub expired: bool,
    pub size_in_bytes: u64,
    pub digest: Option<String>,
    pub workflow_run: Option<ArtifactRun>,
}
#[derive(Deserialize)]
pub(crate) struct ArtifactRun {
    pub id: u64,
    pub head_sha: String,
    pub head_branch: String,
    pub repository_id: u64,
}
#[derive(Deserialize)]
pub(crate) struct ReleaseResponse {
    pub id: u64,
    pub tag_name: String,
    pub draft: Option<bool>,
    pub prerelease: Option<bool>,
    pub immutable: Option<bool>,
    pub body: Option<String>,
    pub author: Option<ReleaseAuthor>,
    pub assets: Vec<ReleaseAssetResponse>,
}
#[derive(Deserialize)]
pub(crate) struct ReleaseAuthor {
    pub login: Option<String>,
    pub id: Option<u64>,
}
#[derive(Deserialize)]
pub(crate) struct ReleaseAssetResponse {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub digest: Option<String>,
}

pub(crate) const GITHUB_ACTIONS_BOT_ID: u64 = 41_898_282;

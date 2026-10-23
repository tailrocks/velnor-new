//! Closed route identities for GitHub Actions evidence reads.

use super::{GitHubRepository, hex_digit};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EvidenceRoute {
    Repository,
    Workflows {
        page: u32,
    },
    WorkflowRuns {
        workflow_id: u64,
        branch: String,
        created: String,
        page: u32,
    },
    WorkflowPredecessor {
        workflow_id: u64,
        branch: String,
        created_before: String,
    },
    RunAttempt {
        run_id: u64,
        attempt: u32,
    },
    Jobs {
        run_id: u64,
        attempt: u32,
        page: u32,
    },
    RunArtifacts {
        run_id: u64,
        page: u32,
    },
    ArtifactById {
        artifact_id: u64,
    },
    ArtifactDownload {
        artifact_id: u64,
    },
}

impl EvidenceRoute {
    pub(super) fn path(&self, repo: &GitHubRepository) -> String {
        let prefix = repo.route_prefix();
        match self {
            Self::Repository => prefix,
            Self::Workflows { page } => {
                format!("{prefix}/actions/workflows?per_page=100&page={page}")
            }
            Self::WorkflowRuns {
                workflow_id,
                branch,
                created,
                page,
            } => format!(
                "{prefix}/actions/workflows/{workflow_id}/runs?branch={}&event=push&created={}&sort=created&direction=desc&per_page=100&page={page}",
                query_encode(branch),
                query_encode(created)
            ),
            Self::WorkflowPredecessor {
                workflow_id,
                branch,
                created_before,
            } => format!(
                "{prefix}/actions/workflows/{workflow_id}/runs?branch={}&event=push&created=..{}&sort=created&direction=desc&per_page=1&page=1",
                query_encode(branch),
                query_encode(created_before)
            ),
            Self::RunAttempt { run_id, attempt } => {
                format!("{prefix}/actions/runs/{run_id}/attempts/{attempt}")
            }
            Self::Jobs {
                run_id,
                attempt,
                page,
            } => format!(
                "{prefix}/actions/runs/{run_id}/attempts/{attempt}/jobs?per_page=100&page={page}"
            ),
            Self::RunArtifacts { run_id, page } => {
                format!("{prefix}/actions/runs/{run_id}/artifacts?per_page=100&page={page}")
            }
            Self::ArtifactById { artifact_id } => {
                format!("{prefix}/actions/artifacts/{artifact_id}")
            }
            Self::ArtifactDownload { artifact_id } => {
                format!("{prefix}/actions/artifacts/{artifact_id}/zip")
            }
        }
    }
}

fn query_encode(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            result.push(char::from(byte));
        } else {
            result.push('%');
            result.push(hex_digit(byte >> 4));
            result.push(hex_digit(byte & 0x0f));
        }
    }
    result
}

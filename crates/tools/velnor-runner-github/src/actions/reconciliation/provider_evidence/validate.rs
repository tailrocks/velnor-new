use std::collections::HashSet;

use super::read::AttemptRunResponse;
use super::types::{
    ActionsWorkflowAttemptEvidenceGap, ActionsWorkflowAttemptJobEvidence,
    ActionsWorkflowAttemptProviderRead, ActionsWorkflowRunArtifactEvidence,
};

pub(super) fn validate_attempt_run(
    run: &AttemptRunResponse,
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    attempt: u32,
    expected_head_sha: &str,
) -> Option<ActionsWorkflowAttemptEvidenceGap> {
    if run.id != workflow_run_id {
        return Some(ActionsWorkflowAttemptEvidenceGap::RunIdMismatch);
    }
    if run.run_attempt != i64::from(attempt) {
        return Some(ActionsWorkflowAttemptEvidenceGap::AttemptMismatch);
    }
    let expected_full_name = format!("{owner}/{repository}");
    if run.repository.id <= 0
        || !run
            .repository
            .full_name
            .eq_ignore_ascii_case(&expected_full_name)
    {
        return Some(ActionsWorkflowAttemptEvidenceGap::RepositoryMismatch);
    }
    if !run.head_sha.eq_ignore_ascii_case(expected_head_sha) {
        return Some(ActionsWorkflowAttemptEvidenceGap::HeadShaMismatch);
    }
    None
}

pub(super) fn validate_job_rows(
    jobs: &[ActionsWorkflowAttemptJobEvidence],
    workflow_run_id: i64,
    head_sha: &str,
) -> Option<ActionsWorkflowAttemptEvidenceGap> {
    let mut ids = HashSet::with_capacity(jobs.len());
    for job in jobs {
        if job.run_id != workflow_run_id || !job.head_sha.eq_ignore_ascii_case(head_sha) {
            return Some(ActionsWorkflowAttemptEvidenceGap::JobIdentityMismatch);
        }
        if !ids.insert(job.id) {
            return Some(ActionsWorkflowAttemptEvidenceGap::DuplicateJobId);
        }
    }
    None
}

pub(super) fn validate_artifact_rows(
    artifacts: &[ActionsWorkflowRunArtifactEvidence],
    workflow_run_id: i64,
    repository_id: i64,
    head_sha: &str,
) -> Option<ActionsWorkflowAttemptEvidenceGap> {
    let mut ids = HashSet::with_capacity(artifacts.len());
    for artifact in artifacts {
        if artifact.workflow_run_id != workflow_run_id
            || artifact.repository_id != repository_id
            || !artifact.head_sha.eq_ignore_ascii_case(head_sha)
        {
            return Some(ActionsWorkflowAttemptEvidenceGap::ArtifactIdentityMismatch);
        }
        if !ids.insert(artifact.id) {
            return Some(ActionsWorkflowAttemptEvidenceGap::DuplicateArtifactId);
        }
    }
    None
}

pub(super) fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}

pub(super) fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn unavailable(
    gap: ActionsWorkflowAttemptEvidenceGap,
) -> ActionsWorkflowAttemptProviderRead {
    ActionsWorkflowAttemptProviderRead::Unavailable(gap)
}

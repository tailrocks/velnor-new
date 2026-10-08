use crate::registration::AsyncDiscoveryTransport;
use crate::{SessionError, WireError};

use super::super::super::validate_repository;
use super::super::MAX_ATTEMPTS;
use super::read::{
    AttemptRunResponse, Pages, Read, decode_artifacts_page, decode_jobs_page, read_all_pages,
    read_attempt_run,
};
use super::types::{
    ActionsWorkflowAttemptEvidenceGap, ActionsWorkflowAttemptJobEvidence,
    ActionsWorkflowAttemptProviderEvidence, ActionsWorkflowAttemptProviderRead,
    ActionsWorkflowRunArtifactEvidence,
};
use super::validate::{
    unavailable, valid_sha, validate_artifact_rows, validate_attempt_run, validate_job_rows,
};

enum ValidatedRows<T> {
    Found(T),
    Unavailable(ActionsWorkflowAttemptEvidenceGap),
}

/// Read the exact workflow attempt, its complete paginated job inventory, and
/// the complete run-scoped artifact inventory using the Actions:read token.
///
/// All calls are read-only `GET`s to fixed repository-scoped Actions REST
/// paths through the host's bounded discovery transport. No artifact archive
/// is downloaded and redirects are not followed by this API. The exact
/// attempt path and `run_attempt` response field must match `attempt`; the
/// attempt head SHA must match `expected_head_sha`; job rows must match that
/// run/SHA; artifact nested run metadata must match the run/repository/SHA.
/// GitHub's unassigned sentinels (`0` IDs and empty runner/group names) are
/// normalized to absent runner identity fields.
/// Each list is capped at four pages of 100, and changing counts, short pages,
/// duplicate IDs, 404s, or identity mismatches return `Unavailable` without
/// partial rows.
///
/// This boundary does not map GitHub job names/artifact IDs to a planner's
/// source/plan/profile/logical-job keys. It also cannot prove an artifact was
/// produced by this particular attempt or job, because the provider artifact
/// object has no attempt/job identity. Higher-level comparison must keep those
/// lanes unproven unless it has a separate exact mapping source.
///
/// # Errors
///
/// Returns an error when request construction, transport, JSON decoding, or
/// response-shape validation fails. HTTP 404 and bounded/inconsistent
/// inventories are returned as typed `Unavailable` outcomes.
pub async fn read_actions_workflow_attempt_provider_evidence_async<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    attempt: u32,
    expected_head_sha: &str,
    actions_token: &str,
) -> Result<ActionsWorkflowAttemptProviderRead, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    if let Some(gap) = validate_provider_request(
        owner,
        repository,
        workflow_run_id,
        attempt,
        expected_head_sha,
        actions_token,
    )? {
        return Ok(unavailable(gap));
    }

    let attempt_run = match read_and_validate_attempt_run(
        transport,
        owner,
        repository,
        workflow_run_id,
        attempt,
        expected_head_sha,
        actions_token,
    )
    .await?
    {
        ValidatedRows::Found(run) => run,
        ValidatedRows::Unavailable(gap) => return Ok(unavailable(gap)),
    };
    let jobs = match read_attempt_jobs(
        transport,
        owner,
        repository,
        workflow_run_id,
        attempt,
        &attempt_run.head_sha,
        actions_token,
    )
    .await?
    {
        ValidatedRows::Found(rows) => rows,
        ValidatedRows::Unavailable(gap) => return Ok(unavailable(gap)),
    };
    let artifacts = match read_run_artifacts(
        transport,
        owner,
        repository,
        workflow_run_id,
        attempt_run.repository.id,
        &attempt_run.head_sha,
        actions_token,
    )
    .await?
    {
        ValidatedRows::Found(rows) => rows,
        ValidatedRows::Unavailable(gap) => return Ok(unavailable(gap)),
    };

    let evidence = into_provider_evidence(attempt_run, workflow_run_id, attempt, jobs, artifacts);
    Ok(ActionsWorkflowAttemptProviderRead::Complete(Box::new(
        evidence,
    )))
}

pub(super) fn validate_provider_request(
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    attempt: u32,
    expected_head_sha: &str,
    actions_token: &str,
) -> Result<Option<ActionsWorkflowAttemptEvidenceGap>, SessionError> {
    validate_repository(owner, repository, actions_token)?;
    if workflow_run_id <= 0 || !valid_sha(expected_head_sha) {
        return Err(WireError::RegistrationRejected.into());
    }
    Ok((attempt == 0 || attempt > MAX_ATTEMPTS)
        .then_some(ActionsWorkflowAttemptEvidenceGap::AttemptLimitExceeded))
}

async fn read_and_validate_attempt_run<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    attempt: u32,
    expected_head_sha: &str,
    token: &str,
) -> Result<ValidatedRows<AttemptRunResponse>, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    let run = match read_attempt_run(
        transport,
        owner,
        repository,
        workflow_run_id,
        attempt,
        token,
    )
    .await?
    {
        Read::Found(run) => run,
        Read::Missing => {
            return Ok(ValidatedRows::Unavailable(
                ActionsWorkflowAttemptEvidenceGap::WorkflowAttemptNotFound,
            ));
        }
    };
    match validate_attempt_run(
        &run,
        owner,
        repository,
        workflow_run_id,
        attempt,
        expected_head_sha,
    ) {
        Some(gap) => Ok(ValidatedRows::Unavailable(gap)),
        None => Ok(ValidatedRows::Found(run)),
    }
}

pub(super) fn into_provider_evidence(
    attempt_run: AttemptRunResponse,
    workflow_run_id: i64,
    attempt: u32,
    jobs: Vec<ActionsWorkflowAttemptJobEvidence>,
    artifacts: Vec<ActionsWorkflowRunArtifactEvidence>,
) -> ActionsWorkflowAttemptProviderEvidence {
    ActionsWorkflowAttemptProviderEvidence {
        repository_id: attempt_run.repository.id,
        repository_full_name: attempt_run.repository.full_name,
        workflow_run_id,
        attempt,
        head_sha: attempt_run.head_sha,
        workflow_path: attempt_run.path,
        event: attempt_run.event,
        head_branch: attempt_run.head_branch,
        head_repository_id: attempt_run.head_repository.as_ref().map(|repo| repo.id),
        head_repository_full_name: attempt_run.head_repository.map(|repo| repo.full_name),
        status: attempt_run.status,
        conclusion: attempt_run.conclusion,
        jobs,
        artifacts,
    }
}

async fn read_attempt_jobs<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    attempt: u32,
    head_sha: &str,
    token: &str,
) -> Result<ValidatedRows<Vec<ActionsWorkflowAttemptJobEvidence>>, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    let path = format!(
        "repos/{owner}/{repository}/actions/runs/{workflow_run_id}/attempts/{attempt}/jobs"
    );
    let jobs = match read_all_pages(transport, &path, token, |body| {
        decode_jobs_page(body, owner, repository)
    })
    .await?
    {
        Pages::Found(jobs) => jobs,
        Pages::Missing => {
            return Ok(ValidatedRows::Unavailable(
                ActionsWorkflowAttemptEvidenceGap::AttemptJobsNotFound,
            ));
        }
        Pages::LimitExceeded => {
            return Ok(ValidatedRows::Unavailable(
                ActionsWorkflowAttemptEvidenceGap::JobPageLimitExceeded,
            ));
        }
        Pages::Inconsistent => {
            return Ok(ValidatedRows::Unavailable(
                ActionsWorkflowAttemptEvidenceGap::InconsistentJobPages,
            ));
        }
    };
    match validate_job_rows(&jobs, workflow_run_id, head_sha) {
        Some(gap) => Ok(ValidatedRows::Unavailable(gap)),
        None => Ok(ValidatedRows::Found(jobs)),
    }
}

async fn read_run_artifacts<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    repository_id: i64,
    head_sha: &str,
    token: &str,
) -> Result<ValidatedRows<Vec<ActionsWorkflowRunArtifactEvidence>>, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    let path = format!("repos/{owner}/{repository}/actions/runs/{workflow_run_id}/artifacts");
    let artifacts = match read_all_pages(transport, &path, token, decode_artifacts_page).await? {
        Pages::Found(artifacts) => artifacts,
        Pages::Missing => {
            return Ok(ValidatedRows::Unavailable(
                ActionsWorkflowAttemptEvidenceGap::RunArtifactsNotFound,
            ));
        }
        Pages::LimitExceeded => {
            return Ok(ValidatedRows::Unavailable(
                ActionsWorkflowAttemptEvidenceGap::ArtifactPageLimitExceeded,
            ));
        }
        Pages::Inconsistent => {
            return Ok(ValidatedRows::Unavailable(
                ActionsWorkflowAttemptEvidenceGap::InconsistentArtifactPages,
            ));
        }
    };
    match validate_artifact_rows(&artifacts, workflow_run_id, repository_id, head_sha) {
        Some(gap) => Ok(ValidatedRows::Unavailable(gap)),
        None => Ok(ValidatedRows::Found(artifacts)),
    }
}

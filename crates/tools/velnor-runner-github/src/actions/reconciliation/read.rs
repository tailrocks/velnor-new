//! Read-only Actions workflow-run and attempt-job endpoints.

use crate::registration::{AsyncDiscoveryTransport, execute_discovery};
use crate::session::execute;
use crate::{ActionsJob, ActionsWorkflowRun, SessionError, Transport, WireError};
use serde::Deserialize;

use super::super::{actions_request, status_error};
use super::PAGE_SIZE;

pub(super) enum Read<T> {
    Found(T),
    Missing,
}

pub(super) fn read_workflow_run<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    run_id: i64,
    token: &str,
) -> Result<Read<ActionsWorkflowRun>, SessionError>
where
    T: Transport + ?Sized,
{
    let request = actions_request(
        format!("repos/{owner}/{repository}/actions/runs/{run_id}"),
        token,
    )?;
    let exchange = execute(transport, &request)?;
    if exchange.status == 404 {
        return Ok(Read::Missing);
    }
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    decode_workflow_run(&exchange.body).map(Read::Found)
}

pub(super) async fn read_workflow_run_async<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    run_id: i64,
    token: &str,
) -> Result<Read<ActionsWorkflowRun>, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    transport.bind_github_api_origin()?;
    let request = actions_request(
        format!("repos/{owner}/{repository}/actions/runs/{run_id}"),
        token,
    )?;
    let exchange = execute_discovery(transport, request).await?;
    if exchange.status == 404 {
        return Ok(Read::Missing);
    }
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    decode_workflow_run(&exchange.body).map(Read::Found)
}

fn decode_workflow_run(body: &[u8]) -> Result<ActionsWorkflowRun, SessionError> {
    let parsed: super::super::WorkflowRunResponse =
        serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    if parsed.id <= 0
        || parsed.run_attempt <= 0
        || parsed.status.is_empty()
        || parsed.event.is_empty()
        || parsed.head_sha.is_empty()
    {
        return Err(WireError::Malformed.into());
    }
    let path = parsed
        .path
        .filter(|path| !path.is_empty())
        .ok_or(WireError::Malformed)?;
    Ok(ActionsWorkflowRun {
        id: parsed.id,
        path,
        run_attempt: parsed.run_attempt,
        status: parsed.status,
        conclusion: parsed.conclusion,
        event: parsed.event,
        head_sha: parsed.head_sha,
        head_repository_full_name: parsed.head_repository.map(|repo| repo.full_name),
    })
}

pub(super) fn attempt_page<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    run_id: i64,
    attempt: u32,
    page: usize,
    token: &str,
) -> Result<Read<AttemptJobs>, SessionError>
where
    T: Transport + ?Sized,
{
    let mut request = actions_request(
        format!("repos/{owner}/{repository}/actions/runs/{run_id}/attempts/{attempt}/jobs"),
        token,
    )?;
    request.query = Some(format!("per_page={PAGE_SIZE}&page={page}"));
    let exchange = execute(transport, &request)?;
    if exchange.status == 404 {
        return Ok(Read::Missing);
    }
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    decode_attempt_jobs(&exchange.body).map(Read::Found)
}

pub(super) async fn attempt_page_async<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    run_id: i64,
    attempt: u32,
    page: usize,
    token: &str,
) -> Result<Read<AttemptJobs>, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    transport.bind_github_api_origin()?;
    let mut request = actions_request(
        format!("repos/{owner}/{repository}/actions/runs/{run_id}/attempts/{attempt}/jobs"),
        token,
    )?;
    request.query = Some(format!("per_page={PAGE_SIZE}&page={page}"));
    let exchange = execute_discovery(transport, request).await?;
    if exchange.status == 404 {
        return Ok(Read::Missing);
    }
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    decode_attempt_jobs(&exchange.body).map(Read::Found)
}

fn decode_attempt_jobs(body: &[u8]) -> Result<AttemptJobs, SessionError> {
    let parsed: AttemptJobsResponse =
        serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    Ok(AttemptJobs {
        total_count: parsed.total_count,
        jobs: parsed.jobs.into_iter().map(ActionsJob::from).collect(),
    })
}

pub(super) struct AttemptJobs {
    pub(super) total_count: usize,
    pub(super) jobs: Vec<ActionsJob>,
}

#[derive(Deserialize)]
struct AttemptJobsResponse {
    total_count: usize,
    jobs: Vec<super::super::JobResponse>,
}

impl From<super::super::JobResponse> for ActionsJob {
    fn from(job: super::super::JobResponse) -> Self {
        Self {
            id: job.id,
            run_id: job.run_id,
            status: job.status,
            conclusion: job.conclusion,
            runner_id: job.runner_id,
            runner_name: job.runner_name,
            runner_group_id: job.runner_group_id,
            runner_group_name: job.runner_group_name,
        }
    }
}

use serde::Deserialize;

use crate::registration::{AsyncDiscoveryTransport, execute_discovery};
use crate::{SessionError, WireError};

use super::super::super::{actions_request, status_error};
use super::super::PAGE_SIZE;
use super::types::{ActionsWorkflowAttemptJobEvidence, ActionsWorkflowRunArtifactEvidence};
use super::validate::{valid_sha, valid_text};

const MAX_PAGES: usize = 4;
const MAX_PROVIDER_ROWS: usize = PAGE_SIZE * MAX_PAGES;

pub(super) enum Read<T> {
    Found(T),
    Missing,
}

pub(super) async fn read_attempt_run<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    attempt: u32,
    token: &str,
) -> Result<Read<AttemptRunResponse>, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    transport.bind_github_api_origin()?;
    let request = actions_request(
        format!("repos/{owner}/{repository}/actions/runs/{workflow_run_id}/attempts/{attempt}"),
        token,
    )?;
    let exchange = execute_discovery(transport, request).await?;
    if exchange.status == 404 {
        return Ok(Read::Missing);
    }
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    let run: AttemptRunResponse =
        serde_json::from_slice(&exchange.body).map_err(|_| WireError::Malformed)?;
    if run.id <= 0
        || run.run_attempt <= 0
        || !valid_text(&run.path)
        || !valid_text(&run.status)
        || !valid_text(&run.event)
        || !valid_sha(&run.head_sha)
        || run.repository.id <= 0
        || !valid_text(&run.repository.full_name)
        || run
            .head_branch
            .as_deref()
            .is_some_and(|value| !valid_text(value))
        || run
            .head_repository
            .as_ref()
            .is_some_and(|repo| repo.id <= 0 || !valid_text(&repo.full_name))
        || run
            .conclusion
            .as_deref()
            .is_some_and(|value| !valid_text(value))
    {
        return Err(WireError::Malformed.into());
    }
    Ok(Read::Found(run))
}

pub(super) enum Pages<T> {
    Found(Vec<T>),
    Missing,
    LimitExceeded,
    Inconsistent,
}

pub(super) struct Page<T> {
    pub(super) total_count: usize,
    pub(super) items: Vec<T>,
}

pub(super) async fn read_all_pages<T, Item, Decode>(
    transport: &mut T,
    path: &str,
    token: &str,
    decode: Decode,
) -> Result<Pages<Item>, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
    Decode: Fn(&[u8]) -> Result<Page<Item>, SessionError>,
{
    let first = match read_page(transport, path, token, 1, &decode).await? {
        Read::Found(page) => page,
        Read::Missing => return Ok(Pages::Missing),
    };
    let pages = first.total_count.div_ceil(PAGE_SIZE).max(1);
    if pages > MAX_PAGES || first.total_count > MAX_PROVIDER_ROWS {
        return Ok(Pages::LimitExceeded);
    }
    let total_count = first.total_count;
    let mut output = Vec::with_capacity(total_count);
    let mut first_page = Some(first);
    for page_number in 1..=pages {
        let page = if let Some(first) = first_page.take() {
            first
        } else {
            match read_page(transport, path, token, page_number, &decode).await? {
                Read::Found(page) => page,
                Read::Missing => return Ok(Pages::Missing),
            }
        };
        let expected_count = total_count
            .saturating_sub((page_number - 1) * PAGE_SIZE)
            .min(PAGE_SIZE);
        if page.total_count != total_count
            || page.items.len() > PAGE_SIZE
            || page.items.len() != expected_count
        {
            return Ok(Pages::Inconsistent);
        }
        output.extend(page.items);
    }
    if output.len() != total_count {
        return Ok(Pages::Inconsistent);
    }
    Ok(Pages::Found(output))
}

async fn read_page<T, Item, Decode>(
    transport: &mut T,
    path: &str,
    token: &str,
    page: usize,
    decode: &Decode,
) -> Result<Read<Page<Item>>, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
    Decode: Fn(&[u8]) -> Result<Page<Item>, SessionError>,
{
    transport.bind_github_api_origin()?;
    let mut request = actions_request(path.to_owned(), token)?;
    request.query = Some(format!("per_page={PAGE_SIZE}&page={page}"));
    let exchange = execute_discovery(transport, request).await?;
    if exchange.status == 404 {
        return Ok(Read::Missing);
    }
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    decode(&exchange.body).map(Read::Found)
}

pub(super) fn decode_jobs_page(
    body: &[u8],
    owner: &str,
    repository: &str,
) -> Result<Page<ActionsWorkflowAttemptJobEvidence>, SessionError> {
    let parsed: JobsPageResponse =
        serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    let items = parsed
        .jobs
        .into_iter()
        .map(|job| {
            let runner_id = normalize_runner_id(job.runner_id)?;
            let runner_name = normalize_runner_name(job.runner_name)?;
            let runner_group_id = normalize_runner_id(job.runner_group_id)?;
            let runner_group_name = normalize_runner_name(job.runner_group_name)?;
            let check_run_id = job
                .check_run_url
                .as_deref()
                .map(|url| parse_check_run_url(url, owner, repository))
                .transpose()?;
            if job.id <= 0
                || job.run_id <= 0
                || !valid_text(&job.name)
                || !valid_sha(&job.head_sha)
                || !valid_text(&job.status)
                || job
                    .conclusion
                    .as_deref()
                    .is_some_and(|value| !valid_text(value))
                || job
                    .workflow_name
                    .as_deref()
                    .is_some_and(|value| !valid_text(value))
                || job
                    .head_branch
                    .as_deref()
                    .is_some_and(|value| !valid_text(value))
                || job.labels.as_ref().is_some_and(|labels| {
                    labels.len() > 256 || labels.iter().any(|value| !valid_text(value))
                })
            {
                return Err(WireError::Malformed.into());
            }
            Ok(ActionsWorkflowAttemptJobEvidence {
                id: job.id,
                check_run_id,
                run_id: job.run_id,
                name: job.name,
                head_sha: job.head_sha,
                status: job.status,
                conclusion: job.conclusion,
                runner_id,
                runner_name,
                runner_group_id,
                runner_group_name,
                workflow_name: job.workflow_name,
                head_branch: job.head_branch,
                labels: job.labels,
            })
        })
        .collect::<Result<Vec<_>, SessionError>>()?;
    Ok(Page {
        total_count: parsed.total_count,
        items,
    })
}

fn normalize_runner_id(value: Option<i64>) -> Result<Option<i64>, SessionError> {
    match value {
        None | Some(0) => Ok(None),
        Some(value) if value > 0 => Ok(Some(value)),
        Some(_) => Err(WireError::Malformed.into()),
    }
}

fn normalize_runner_name(value: Option<String>) -> Result<Option<String>, SessionError> {
    match value {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) if valid_text(&value) => Ok(Some(value)),
        Some(_) => Err(WireError::Malformed.into()),
    }
}

fn parse_check_run_url(url: &str, owner: &str, repository: &str) -> Result<i64, SessionError> {
    let scoped_path = url
        .strip_prefix("https://api.github.com/repos/")
        .ok_or(WireError::Malformed)?;
    let (url_owner, remainder) = scoped_path.split_once('/').ok_or(WireError::Malformed)?;
    let (url_repository, check_run_path) = remainder.split_once('/').ok_or(WireError::Malformed)?;
    if !url_owner.eq_ignore_ascii_case(owner) || !url_repository.eq_ignore_ascii_case(repository) {
        return Err(WireError::Malformed.into());
    }
    let id = check_run_path
        .strip_prefix("check-runs/")
        .ok_or(WireError::Malformed)?;
    if id.is_empty() || id.starts_with('0') || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(WireError::Malformed.into());
    }
    let id = id.parse::<i64>().map_err(|_| WireError::Malformed)?;
    if id <= 0 {
        return Err(WireError::Malformed.into());
    }
    Ok(id)
}

pub(super) fn decode_artifacts_page(
    body: &[u8],
) -> Result<Page<ActionsWorkflowRunArtifactEvidence>, SessionError> {
    let parsed: ArtifactsPageResponse =
        serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    let items = parsed
        .artifacts
        .into_iter()
        .map(|artifact| {
            let run = artifact.workflow_run.ok_or(WireError::Malformed)?;
            if artifact.id <= 0
                || !valid_text(&artifact.name)
                || artifact.size_in_bytes > i64::MAX as u64
                || artifact
                    .digest
                    .as_deref()
                    .is_some_and(|value| !valid_text(value))
                || run.id <= 0
                || run.repository_id <= 0
                || run.head_repository_id.is_some_and(|value| value <= 0)
                || !valid_sha(&run.head_sha)
                || run
                    .head_branch
                    .as_deref()
                    .is_some_and(|value| !valid_text(value))
            {
                return Err(WireError::Malformed.into());
            }
            Ok(ActionsWorkflowRunArtifactEvidence {
                id: artifact.id,
                name: artifact.name,
                size_in_bytes: artifact.size_in_bytes,
                expired: artifact.expired,
                digest: artifact.digest,
                workflow_run_id: run.id,
                repository_id: run.repository_id,
                head_repository_id: run.head_repository_id,
                head_branch: run.head_branch,
                head_sha: run.head_sha,
            })
        })
        .collect::<Result<Vec<_>, SessionError>>()?;
    Ok(Page {
        total_count: parsed.total_count,
        items,
    })
}

#[derive(Deserialize)]
pub(super) struct AttemptRunResponse {
    pub(super) id: i64,
    pub(super) run_attempt: i64,
    pub(super) path: String,
    pub(super) status: String,
    pub(super) conclusion: Option<String>,
    pub(super) event: String,
    pub(super) head_sha: String,
    pub(super) head_branch: Option<String>,
    pub(super) repository: RunRepositoryResponse,
    pub(super) head_repository: Option<RunRepositoryResponse>,
}

#[derive(Deserialize)]
pub(super) struct RunRepositoryResponse {
    pub(super) id: i64,
    pub(super) full_name: String,
}

#[derive(Deserialize)]
struct JobsPageResponse {
    total_count: usize,
    jobs: Vec<JobResponse>,
}

#[derive(Deserialize)]
struct JobResponse {
    pub(super) id: i64,
    check_run_url: Option<String>,
    run_id: i64,
    name: String,
    head_sha: String,
    status: String,
    conclusion: Option<String>,
    runner_id: Option<i64>,
    runner_name: Option<String>,
    runner_group_id: Option<i64>,
    runner_group_name: Option<String>,
    workflow_name: Option<String>,
    head_branch: Option<String>,
    labels: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct ArtifactsPageResponse {
    total_count: usize,
    artifacts: Vec<ArtifactResponse>,
}

#[derive(Deserialize)]
struct ArtifactResponse {
    pub(super) id: i64,
    name: String,
    size_in_bytes: u64,
    expired: bool,
    digest: Option<String>,
    workflow_run: Option<ArtifactRunResponse>,
}

#[derive(Deserialize)]
struct ArtifactRunResponse {
    pub(super) id: i64,
    repository_id: i64,
    head_repository_id: Option<i64>,
    head_branch: Option<String>,
    head_sha: String,
}

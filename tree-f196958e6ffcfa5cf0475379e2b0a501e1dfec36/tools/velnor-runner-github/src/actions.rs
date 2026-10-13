//! Read-only GitHub Actions REST lookups used to reconcile Scale Set messages.

mod fork_workflows;
mod reconciliation;
pub(crate) mod trust;

pub use fork_workflows::{
    ForkPullRequestWorkflowSetting, PrivateRepoForkWorkflowSettings,
    get_private_repo_fork_workflow_settings, get_private_repo_fork_workflow_settings_async,
};
pub use reconciliation::{
    ActionsJobReconciliation, ActionsJobReconciliationReason, ActionsJobReconciliationState,
    ObservedScaleSetJob, reconcile_observed_scale_set_job, reconcile_observed_scale_set_job_async,
};

use serde::Deserialize;

use crate::registration::{AsyncDiscoveryTransport, execute_discovery};
use crate::session::execute;
use crate::{Method, SessionError, SessionRequest, Transport, WireError};

pub(crate) const API_VERSION: &str = "2026-03-10";
const ACCEPT: &str = "application/vnd.github+json";

/// Repository identity and visibility facts from the repository REST endpoint.
/// This DTO is evidence only; callers decide whether the returned policy is
/// sufficient for their trust boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsRepository {
    /// Numeric GitHub repository ID.
    pub id: i64,
    /// Canonical `owner/repository` identity.
    pub full_name: String,
    /// Whether GitHub reports this repository as private.
    pub private: bool,
    /// Whether GitHub reports that the authenticated actor has repository
    /// administrator access. `None` means the endpoint omitted permissions;
    /// discovery callers must not treat absence as authorization.
    pub admin: Option<bool>,
}

/// GitHub Actions job state. `id` is the numeric REST job id, distinct from a
/// Scale Set message's opaque string `jobId`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsJob {
    /// Numeric Actions job id.
    pub id: i64,
    /// Workflow run id reported by the REST job endpoint.
    pub run_id: i64,
    /// Current REST status, such as `queued`, `in_progress`, or `completed`.
    pub status: String,
    /// Terminal result when completed.
    pub conclusion: Option<String>,
    /// GitHub runner id, if assigned.
    pub runner_id: Option<i64>,
    /// GitHub runner name, if assigned.
    pub runner_name: Option<String>,
    /// Runner group id, if reported.
    pub runner_group_id: Option<i64>,
    /// Runner group name, if reported.
    pub runner_group_name: Option<String>,
}

/// Workflow run metadata used for trust and attempt correlation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsWorkflowRun {
    /// Numeric workflow run id.
    pub id: i64,
    /// Workflow file path reported by the workflow-run REST response.
    pub path: String,
    /// Run attempt, starting at one.
    pub run_attempt: i64,
    /// Current REST status.
    pub status: String,
    /// Terminal result when completed.
    pub conclusion: Option<String>,
    /// Trigger event name.
    pub event: String,
    /// Commit SHA for the run.
    pub head_sha: String,
    /// Full name of the source repository, absent when GitHub reports null.
    pub head_repository_full_name: Option<String>,
}

/// Read repository identity and privacy metadata.
///
/// The caller's host-only REST credential needs repository `Metadata:read`
/// (fine-grained GitHub App/PAT) or `repo` (classic token) for private repos.
/// The returned fields are evidence only; callers must compare the returned
/// identity and privacy with their configured trust policy.
///
/// # Errors
///
/// Returns an error if the input is invalid, transport or GitHub API access
/// fails, or the response omits or mismatches the required repository facts.
pub fn get_actions_repository<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    rest_token: &str,
) -> Result<ActionsRepository, SessionError>
where
    T: Transport + ?Sized,
{
    let request = repository_request(owner, repository, rest_token)?;
    let exchange = execute(transport, &request)?;
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    decode_repository(&exchange.body, owner, repository)
}

/// Asynchronously read repository identity and privacy metadata through the
/// host's bounded discovery worker.
///
/// # Errors
///
/// Returns an error if the input is invalid, transport or GitHub API access
/// fails, or the response omits or mismatches required repository facts.
pub async fn get_actions_repository_async<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    rest_token: &str,
) -> Result<ActionsRepository, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    transport.bind_github_api_origin()?;
    let request = repository_request(owner, repository, rest_token)?;
    let exchange = execute_discovery(transport, request).await?;
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    decode_repository(&exchange.body, owner, repository)
}

pub(crate) fn repository_request(
    owner: &str,
    repository: &str,
    rest_token: &str,
) -> Result<SessionRequest, SessionError> {
    validate_repository(owner, repository, rest_token)?;
    actions_request(format!("repos/{owner}/{repository}"), rest_token)
}

pub(crate) fn decode_repository(
    body: &[u8],
    owner: &str,
    repository: &str,
) -> Result<ActionsRepository, SessionError> {
    let parsed: ActionsRepositoryResponse =
        serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    let expected_full_name = format!("{owner}/{repository}");
    let private = parsed.private.ok_or(WireError::Malformed)?;
    if parsed.id <= 0 || !parsed.full_name.eq_ignore_ascii_case(&expected_full_name) {
        return Err(WireError::Malformed.into());
    }
    Ok(ActionsRepository {
        id: parsed.id,
        full_name: parsed.full_name,
        private,
        admin: parsed.permissions.and_then(|permissions| permissions.admin),
    })
}

/// Read an Actions job using the REST job endpoint.
///
/// Scale Set `jobId` is an opaque string in the pinned message contract. This
/// helper only maps a positive decimal string to the numeric REST endpoint;
/// an unsupported opaque value returns `Ok(None)` without making a request.
/// Keep the original Scale Set identifier separately for logs and state.
/// `actions_token` is a separate GitHub REST credential with Actions:read; it
/// must not be the Scale Set service token or a workflow credential.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for invalid path segments or
/// an empty token, [`WireError::Malformed`] for an invalid response, and
/// [`WireError::Forbidden`] when GitHub rejects authentication or permission.
pub fn get_actions_job<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    opaque_job_id: &str,
    actions_token: &str,
) -> Result<Option<ActionsJob>, SessionError>
where
    T: Transport + ?Sized,
{
    validate_repository(owner, repository, actions_token)?;
    let Some(job_id) = numeric_id(opaque_job_id) else {
        return Ok(None);
    };
    let request = actions_request(
        format!("repos/{owner}/{repository}/actions/jobs/{job_id}"),
        actions_token,
    )?;
    let exchange = execute(transport, &request)?;
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    let parsed: JobResponse =
        serde_json::from_slice(&exchange.body).map_err(|_| WireError::Malformed)?;
    if parsed.id != job_id || parsed.run_id <= 0 || parsed.status.is_empty() {
        return Err(WireError::Malformed.into());
    }
    Ok(Some(ActionsJob {
        id: parsed.id,
        run_id: parsed.run_id,
        status: parsed.status,
        conclusion: parsed.conclusion,
        runner_id: parsed.runner_id,
        runner_name: parsed.runner_name,
        runner_group_id: parsed.runner_group_id,
        runner_group_name: parsed.runner_group_name,
    }))
}

/// Read a workflow run to validate event, source repository, SHA, and attempt.
///
/// Call this before acquiring a `JobAvailable` request when enforcing the
/// trusted-repository or no-forks policy. A missing `head_repository` must be
/// treated as insufficient trust evidence by the caller. The REST token is
/// read-only and distinct from the Scale Set service credential.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for invalid input,
/// [`WireError::Malformed`] for an invalid response, and
/// [`WireError::Forbidden`] when GitHub rejects authentication or permission.
pub fn get_actions_workflow_run<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    actions_token: &str,
) -> Result<ActionsWorkflowRun, SessionError>
where
    T: Transport + ?Sized,
{
    validate_repository(owner, repository, actions_token)?;
    if workflow_run_id <= 0 {
        return Err(WireError::RegistrationRejected.into());
    }
    let request = actions_request(
        format!("repos/{owner}/{repository}/actions/runs/{workflow_run_id}"),
        actions_token,
    )?;
    let exchange = execute(transport, &request)?;
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    let parsed: WorkflowRunResponse =
        serde_json::from_slice(&exchange.body).map_err(|_| WireError::Malformed)?;
    if parsed.id != workflow_run_id
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

fn validate_repository(
    owner: &str,
    repository: &str,
    actions_token: &str,
) -> Result<(), SessionError> {
    if !path_segment(owner) || !path_segment(repository) || !safe_actions_token(actions_token) {
        return Err(WireError::RegistrationRejected.into());
    }
    Ok(())
}

fn path_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn numeric_id(value: &str) -> Option<i64> {
    if value.is_empty() || value.len() > 19 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<i64>().ok().filter(|id| *id > 0)
}

pub(crate) fn actions_request(
    path: String,
    actions_token: &str,
) -> Result<SessionRequest, SessionError> {
    if !safe_actions_token(actions_token) {
        return Err(WireError::RegistrationRejected.into());
    }
    Ok(SessionRequest {
        method: Method::Get,
        path,
        query: None,
        headers: vec![
            ("Accept".to_owned(), ACCEPT.to_owned()),
            (
                "Authorization".to_owned(),
                format!("Bearer {actions_token}"),
            ),
            ("X-GitHub-Api-Version".to_owned(), API_VERSION.to_owned()),
            ("User-Agent".to_owned(), "velnor-host".to_owned()),
        ],
        body: Vec::new(),
    })
}

fn safe_actions_token(token: &str) -> bool {
    !token.is_empty()
        && !token
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

pub(crate) fn status_error(status: u16) -> SessionError {
    if status == 401 || status == 403 {
        WireError::Forbidden.into()
    } else {
        WireError::UnexpectedStatus.into()
    }
}

#[derive(Deserialize)]
struct ActionsRepositoryResponse {
    id: i64,
    full_name: String,
    private: Option<bool>,
    permissions: Option<RepositoryPermissions>,
}

#[derive(Deserialize)]
struct RepositoryPermissions {
    admin: Option<bool>,
}

#[derive(Deserialize)]
struct JobResponse {
    id: i64,
    run_id: i64,
    status: String,
    conclusion: Option<String>,
    runner_id: Option<i64>,
    runner_name: Option<String>,
    runner_group_id: Option<i64>,
    runner_group_name: Option<String>,
}

#[derive(Deserialize)]
struct WorkflowRunResponse {
    id: i64,
    path: Option<String>,
    run_attempt: i64,
    status: String,
    conclusion: Option<String>,
    event: String,
    head_sha: String,
    head_repository: Option<RepositoryResponse>,
}

#[derive(Deserialize)]
struct RepositoryResponse {
    full_name: String,
}

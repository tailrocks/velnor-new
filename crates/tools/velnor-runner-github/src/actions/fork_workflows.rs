//! Private-repository fork-workflow controls from GitHub's Actions REST API.

use serde::Deserialize;

use super::{actions_request, status_error, validate_repository};
use crate::registration::{AsyncDiscoveryTransport, execute_discovery};
use crate::session::execute;
use crate::{SessionError, SessionRequest, Transport, WireError};

/// Whether private-repository fork pull-request workflows are enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkPullRequestWorkflowSetting {
    /// GitHub allows workflows from fork pull requests to run.
    Enabled,
    /// GitHub blocks workflows from fork pull requests.
    Disabled,
}

/// Private-repository fork workflow settings from GitHub's Actions REST API.
/// These settings are policy inputs, not a standalone runner authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateRepoForkWorkflowSettings {
    /// Whether workflows from fork pull requests may run.
    pub run_workflows_from_fork_pull_requests: ForkPullRequestWorkflowSetting,
    /// Whether fork workflows receive write tokens.
    pub send_write_tokens_to_workflows: bool,
    /// Whether fork workflows receive secrets and variables.
    pub send_secrets_and_variables: bool,
    /// Whether fork pull request workflows require approval.
    pub require_approval_for_fork_pr_workflows: bool,
}

/// Read the four private-repository fork-workflow controls.
///
/// The caller's host-only REST credential needs repository `Administration:read`
/// (fine-grained GitHub App/PAT) or `repo` (classic token). This method returns
/// raw settings; it does not decide whether they authorize runner admission.
///
/// # Errors
///
/// Returns an error if the input is invalid, transport or GitHub API access
/// fails, or any required policy field is missing from the response.
pub fn get_private_repo_fork_workflow_settings<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    rest_token: &str,
) -> Result<PrivateRepoForkWorkflowSettings, SessionError>
where
    T: Transport + ?Sized,
{
    let request = fork_workflow_settings_request(owner, repository, rest_token)?;
    let exchange = execute(transport, &request)?;
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    decode_fork_workflow_settings(&exchange.body)
}

/// Asynchronously read the private-repository fork-workflow controls through
/// the host's bounded discovery worker.
///
/// # Errors
///
/// Returns an error if the input is invalid, transport or GitHub API access
/// fails, or a required policy field is absent.
pub async fn get_private_repo_fork_workflow_settings_async<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    rest_token: &str,
) -> Result<PrivateRepoForkWorkflowSettings, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    transport.bind_github_api_origin()?;
    let request = fork_workflow_settings_request(owner, repository, rest_token)?;
    let exchange = execute_discovery(transport, request).await?;
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    decode_fork_workflow_settings(&exchange.body)
}

pub(crate) fn fork_workflow_settings_request(
    owner: &str,
    repository: &str,
    rest_token: &str,
) -> Result<SessionRequest, SessionError> {
    validate_repository(owner, repository, rest_token)?;
    actions_request(
        format!("repos/{owner}/{repository}/actions/permissions/fork-pr-workflows-private-repos"),
        rest_token,
    )
}

pub(crate) fn decode_fork_workflow_settings(
    body: &[u8],
) -> Result<PrivateRepoForkWorkflowSettings, SessionError> {
    if body.len() > 512 * 1024 {
        return Err(WireError::Malformed.into());
    }
    let parsed: ForkWorkflowSettingsResponse =
        serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    Ok(PrivateRepoForkWorkflowSettings {
        run_workflows_from_fork_pull_requests: if parsed
            .run_workflows_from_fork_pull_requests
            .ok_or(WireError::Malformed)?
        {
            ForkPullRequestWorkflowSetting::Enabled
        } else {
            ForkPullRequestWorkflowSetting::Disabled
        },
        send_write_tokens_to_workflows: parsed
            .send_write_tokens_to_workflows
            .ok_or(WireError::Malformed)?,
        send_secrets_and_variables: parsed
            .send_secrets_and_variables
            .ok_or(WireError::Malformed)?,
        require_approval_for_fork_pr_workflows: parsed
            .require_approval_for_fork_pr_workflows
            .ok_or(WireError::Malformed)?,
    })
}

#[derive(Deserialize)]
struct ForkWorkflowSettingsResponse {
    run_workflows_from_fork_pull_requests: Option<bool>,
    send_write_tokens_to_workflows: Option<bool>,
    send_secrets_and_variables: Option<bool>,
    require_approval_for_fork_pr_workflows: Option<bool>,
}

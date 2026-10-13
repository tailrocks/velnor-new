use std::time::SystemTime;

use crate::registration::AsyncDiscoveryTransport;
use crate::{
    ForkPullRequestWorkflowSetting, SessionError, Transport, WireError, get_actions_repository,
    get_actions_repository_async, get_private_repo_fork_workflow_settings,
    get_private_repo_fork_workflow_settings_async,
};

/// Read-only repository facts used by the organization pool policy verifier.
///
/// Fields are private and the type is not cloneable. It can only be issued by
/// [`read_repository_pool_trust`] after both fixed-path REST reads succeed.
#[derive(Debug, PartialEq, Eq)]
pub struct RepositoryPoolTrustEvidence {
    pub(super) repository_id: i64,
    pub(super) repository_full_name: String,
    pub(super) private: bool,
    pub(super) forks_disabled: bool,
    pub(super) repository_observed_at: SystemTime,
    pub(super) fork_policy_observed_at: SystemTime,
}

struct RepositoryReadFacts {
    repository_id: i64,
    repository_full_name: String,
    private: bool,
    fork_setting: ForkPullRequestWorkflowSetting,
    repository_observed_at: SystemTime,
    fork_policy_observed_at: SystemTime,
}

impl RepositoryPoolTrustEvidence {
    #[cfg(test)]
    pub(crate) fn from_test_parts(
        repository_id: i64,
        repository_full_name: String,
        private: bool,
        forks_disabled: bool,
        repository_observed_at: SystemTime,
        fork_policy_observed_at: SystemTime,
    ) -> Self {
        Self {
            repository_id,
            repository_full_name,
            private,
            forks_disabled,
            repository_observed_at,
            fork_policy_observed_at,
        }
    }
}

/// Read the exact repository identity/privacy and private-fork workflow setting.
///
/// The caller must bind `transport` to the fixed GitHub REST origin and use a
/// host-only read credential. This performs only the repository metadata GET
/// and private fork-workflow settings GET. The returned evidence does not
/// establish that a runner group routes this repository.
///
/// # Errors
///
/// Returns a secret-safe error if either response is unavailable, malformed,
/// or denied. No error contains response bodies or credentials.
pub fn read_repository_pool_trust<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    actions_token: &str,
) -> Result<RepositoryPoolTrustEvidence, SessionError>
where
    T: Transport + ?Sized,
{
    let repository_evidence = get_actions_repository(transport, owner, repository, actions_token)?;
    let repository_observed_at = SystemTime::now();
    let fork_settings =
        get_private_repo_fork_workflow_settings(transport, owner, repository, actions_token)?;
    let fork_policy_observed_at = SystemTime::now();
    build_repository_pool_trust(
        RepositoryReadFacts {
            repository_id: repository_evidence.id,
            repository_full_name: repository_evidence.full_name,
            private: repository_evidence.private,
            fork_setting: fork_settings.run_workflows_from_fork_pull_requests,
            repository_observed_at,
            fork_policy_observed_at,
        },
        owner,
        repository,
    )
}

/// Read repository identity/privacy and fork-workflow policy with the host's
/// bounded async transport. The transport binds GitHub's fixed API origin for
/// each request and does not retry or follow redirects.
///
/// # Errors
///
/// Returns secret-safe errors if the repository or fork-policy read fails,
/// is incomplete, or identifies another repository.
pub async fn read_repository_pool_trust_async<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    actions_token: &str,
) -> Result<RepositoryPoolTrustEvidence, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    let repository_evidence =
        get_actions_repository_async(transport, owner, repository, actions_token).await?;
    let repository_observed_at = SystemTime::now();
    let fork_settings =
        get_private_repo_fork_workflow_settings_async(transport, owner, repository, actions_token)
            .await?;
    let fork_policy_observed_at = SystemTime::now();
    build_repository_pool_trust(
        RepositoryReadFacts {
            repository_id: repository_evidence.id,
            repository_full_name: repository_evidence.full_name,
            private: repository_evidence.private,
            fork_setting: fork_settings.run_workflows_from_fork_pull_requests,
            repository_observed_at,
            fork_policy_observed_at,
        },
        owner,
        repository,
    )
}

fn build_repository_pool_trust(
    read: RepositoryReadFacts,
    owner: &str,
    repository: &str,
) -> Result<RepositoryPoolTrustEvidence, SessionError> {
    let expected_full_name = format!("{owner}/{repository}");
    if read.repository_id <= 0
        || !read
            .repository_full_name
            .eq_ignore_ascii_case(&expected_full_name)
    {
        return Err(WireError::Malformed.into());
    }
    Ok(RepositoryPoolTrustEvidence {
        repository_id: read.repository_id,
        repository_full_name: read.repository_full_name,
        private: read.private,
        forks_disabled: read.fork_setting == ForkPullRequestWorkflowSetting::Disabled,
        repository_observed_at: read.repository_observed_at,
        fork_policy_observed_at: read.fork_policy_observed_at,
    })
}

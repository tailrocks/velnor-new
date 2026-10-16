//! Mode and save inputs for MBX object caching governed by PR cache policy.

use velnor_actions_contract::PullRequestCachePolicy;

/// Write permission for read-only PR policy (protected default-branch pushes only).
pub(crate) const CACHE_MODE_READ_ONLY: &str = "${{ github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && 'write' || 'read' }}";

/// Write permission for same-repository scoped PR policy (push to default branch or same-repo PR).
pub(crate) const CACHE_MODE_SAME_REPO: &str = "${{ ((github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true) || (github.event_name == 'pull_request' && !github.event.pull_request.head.repo.fork)) && 'write' || 'read' }}";

/// Return the `ACTIONS_CACHE_MODE` expression for the given PR cache policy.
#[must_use]
pub(crate) fn mbx_cache_mode_value(policy: PullRequestCachePolicy) -> &'static str {
    match policy {
        PullRequestCachePolicy::ReadOnly => CACHE_MODE_READ_ONLY,
        PullRequestCachePolicy::SameRepositoryScoped => CACHE_MODE_SAME_REPO,
    }
}

/// Return the `save-on-pull-request` value for the given PR cache policy.
#[must_use]
pub(crate) fn mbx_save_on_pull_request_value(policy: PullRequestCachePolicy) -> &'static str {
    match policy {
        PullRequestCachePolicy::ReadOnly => "false",
        PullRequestCachePolicy::SameRepositoryScoped => {
            "${{ github.event_name == 'pull_request' && !github.event.pull_request.head.repo.fork }}"
        }
    }
}

/// Check if the provided cache mode value is valid for any supported policy.
#[must_use]
pub(crate) fn is_valid_mbx_cache_mode(value: &str) -> bool {
    value == CACHE_MODE_READ_ONLY || value == CACHE_MODE_SAME_REPO
}

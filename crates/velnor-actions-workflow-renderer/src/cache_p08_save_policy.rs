//! Trust gates for tool-cache writes in generated consumer workflows.
//!
//! Every gate also requires the restore to have missed the exact key: a
//! byte-identical entry is never written again (a PR ref would otherwise
//! re-store the default branch's exact tools entry under its own scope).

use velnor_actions_contract::PullRequestCachePolicy;

/// Exact-key miss of the job's V2 tools restore.
pub(crate) const TOOLS_RESTORE_MISS: &str = "steps.tools-restore.outputs.cache-hit != 'true'";

const SAVE_GATES: [&str; 6] = [
    "success()",
    "github.event_name == 'push'",
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)",
    "github.ref_protected == true",
    "steps.v2.outputs.enabled == 'true'",
    TOOLS_RESTORE_MISS,
];

const PR_SCOPED_SAVE_CONDITION: &str = "success() && ((github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true) || (github.event_name == 'pull_request' && !github.event.pull_request.head.repo.fork)) && steps.v2.outputs.enabled == 'true' && steps.tools-restore.outputs.cache-hit != 'true'";

/// Build the GitHub expression for a V2 tools cache save under `policy`.
pub(super) fn condition(policy: PullRequestCachePolicy) -> String {
    match policy {
        PullRequestCachePolicy::ReadOnly => SAVE_GATES.join(" && "),
        PullRequestCachePolicy::SameRepositoryScoped => PR_SCOPED_SAVE_CONDITION.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_tools_save_gate_requires_an_exact_restore_miss() {
        for policy in [
            super::PullRequestCachePolicy::ReadOnly,
            super::PullRequestCachePolicy::SameRepositoryScoped,
        ] {
            assert!(
                super::condition(policy).ends_with(super::TOOLS_RESTORE_MISS),
                "{policy:?}"
            );
        }
    }
}

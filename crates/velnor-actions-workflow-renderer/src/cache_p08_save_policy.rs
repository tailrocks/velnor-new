//! Trust gates for tool-cache writes in generated consumer workflows.

use velnor_actions_contract::PullRequestCachePolicy;

const SAVE_GATES: [&str; 5] = [
    "success()",
    "github.event_name == 'push'",
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)",
    "github.ref_protected == true",
    "steps.v2.outputs.enabled == 'true'",
];

const PR_SCOPED_SAVE_CONDITION: &str = "success() && ((github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true) || (github.event_name == 'pull_request' && !github.event.pull_request.head.repo.fork)) && steps.v2.outputs.enabled == 'true'";

/// Build the GitHub expression for a V2 tools cache save under `policy`.
pub(super) fn condition(policy: PullRequestCachePolicy) -> String {
    match policy {
        PullRequestCachePolicy::ReadOnly => SAVE_GATES.join(" && "),
        PullRequestCachePolicy::SameRepositoryScoped => PR_SCOPED_SAVE_CONDITION.to_owned(),
    }
}

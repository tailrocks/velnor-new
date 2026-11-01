// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::PullRequestCachePolicy;

pub(crate) const TOOLS_RESTORE_MISS: &str = "steps.tools-restore.outputs.cache-hit != 'true'";

const SAVE_GATES: [&str; 6] = [
    "success()",
    "github.event_name == 'push'",
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)",
    "github.ref_protected == true",
    "steps.v2.outputs.enabled == 'true'",
    TOOLS_RESTORE_MISS,
];

const PUSH_GATE: &str = "github.event_name == 'push'";

const SCHEDULED_GATE: &str =
    "(github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')";

const PR_SCOPED_SAVE_CONDITION: &str = "success() && ((github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true) || (github.event_name == 'pull_request' && !github.event.pull_request.head.repo.fork)) && steps.v2.outputs.enabled == 'true' && steps.tools-restore.outputs.cache-hit != 'true'";

pub(super) fn condition(policy: PullRequestCachePolicy) -> String {
    match policy {
        PullRequestCachePolicy::ReadOnly => SAVE_GATES.join(" && "),
        PullRequestCachePolicy::SameRepositoryScoped => PR_SCOPED_SAVE_CONDITION.to_owned(),
    }
}

pub(super) fn scheduled_condition() -> String {
    SAVE_GATES
        .map(|gate| {
            if gate == PUSH_GATE {
                SCHEDULED_GATE
            } else {
                gate
            }
        })
        .join(" && ")
}

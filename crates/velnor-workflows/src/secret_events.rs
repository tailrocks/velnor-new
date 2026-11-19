// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expression::bare;
use crate::plan::Triggers;

pub(crate) const DEFAULT_BRANCH_GATE: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

/// The events of a workflow that an author of a pull request, a caller or a follower of
/// another workflow can start, and a push to a branch that no condition fences to the
/// default branch. A secret is read only when this list is empty.
pub(crate) fn untrusted(triggers: &Triggers, conditions: [Option<&str>; 2]) -> Vec<&'static str> {
    let fenced = conditions.into_iter().flatten().any(fences_default_branch);
    triggers
        .events()
        .into_iter()
        .filter(|event| match *event {
            "workflow_dispatch" | "schedule" => false,
            "push" => {
                !fenced
                    && triggers
                        .push
                        .as_ref()
                        .is_none_or(|push| !push.branches.is_empty())
            }
            _ => true,
        })
        .collect()
}

fn fences_default_branch(condition: &str) -> bool {
    let condition = bare(condition);
    if condition.contains("||") {
        return false;
    }
    condition
        .split(" && ")
        .any(|part| part.trim() == DEFAULT_BRANCH_GATE)
        && condition
            .split(" && ")
            .all(|part| part.matches('\'').count() % 2 == 0)
}

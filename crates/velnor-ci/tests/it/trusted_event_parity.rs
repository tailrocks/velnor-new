// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::mbx_store::trusted_event;
use velnor_model::TRUSTED_EVENT;
use velnor_test_support::gh_expr::{Value, evaluate};

const DEFAULT_BRANCH: &str = "main";

const TABLE: [(&str, &str); 22] = [
    ("push", "refs/heads/main"),
    ("push", "refs/heads/feature"),
    ("push", "refs/heads/main-next"),
    ("push", "refs/heads/release/main"),
    ("push", "refs/tags/main"),
    ("push", "refs/tags/v1.0.0"),
    ("schedule", "refs/heads/main"),
    ("workflow_dispatch", "refs/heads/main"),
    ("workflow_dispatch", "refs/heads/feature"),
    ("workflow_dispatch", "refs/tags/v1.0.0"),
    ("pull_request", "refs/pull/7/merge"),
    ("pull_request", "refs/heads/main"),
    ("pull_request_target", "refs/heads/main"),
    ("pull_request_review", "refs/heads/main"),
    ("merge_group", "refs/heads/gh-readonly-queue/main/pr-7"),
    ("merge_group", "refs/heads/main"),
    ("workflow_run", "refs/heads/main"),
    ("issue_comment", "refs/heads/main"),
    ("repository_dispatch", "refs/heads/main"),
    ("release", "refs/heads/main"),
    ("create", "refs/heads/main"),
    ("delete", "refs/heads/main"),
];

fn expression_trusts(
    event: &str,
    git_ref: &str,
    default_branch: Option<&str>,
) -> Result<bool, String> {
    let resolve = |name: &str| -> Result<Value, String> {
        match name {
            "github.event_name" => Ok(Value::Str(event.to_owned())),
            "github.ref" => Ok(Value::Str(git_ref.to_owned())),
            "github.event.repository.default_branch" => {
                Ok(default_branch.map_or(Value::Null, |branch| Value::Str(branch.to_owned())))
            }
            other => Err(format!("unknown context {other}")),
        }
    };
    Ok(evaluate(TRUSTED_EVENT, &resolve)?.truthy())
}

#[test]
fn the_routing_expression_and_the_helper_trust_the_same_events() -> Result<(), String> {
    for default_branch in [Some(DEFAULT_BRANCH), None, Some("")] {
        for (event, git_ref) in TABLE {
            assert_eq!(
                expression_trusts(event, git_ref, default_branch)?,
                trusted_event(event, git_ref, default_branch),
                "{event} {git_ref} {default_branch:?}"
            );
        }
    }
    Ok(())
}

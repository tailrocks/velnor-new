// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_runner_github::workflow_run::{
    ComparedHookSourceFacts, HookSourceError, RepositoryTarget, UntrustedHookSourceInput,
    compare_hook_source,
};

use super::common::{Script, exchange};

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DEPLOY_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DEPLOY: &str = "owner/repo/.github/workflows/deploy.yml@refs/heads/main";

fn input_bytes(event_name: &str, event_ref: &str) -> Vec<u8> {
    json!({
        "repository_id": 7_i64,
        "repository": "owner/repo",
        "run_id": 9001_i64,
        "run_attempt": 1_i64,
        "event_name": event_name,
        "event_ref": event_ref,
        "workflow_ref": format!("owner/repo/.github/workflows/ci.yml@{event_ref}"),
        "workflow_sha": SHA,
        "tested_sha": SHA,
        "job_workflow_ref": DEPLOY,
        "workflow_job_id": "deploy"
    })
    .to_string()
    .into_bytes()
}

fn compare(
    event_name: &str,
    event_ref: &str,
    head_branch: Option<&str>,
) -> Result<ComparedHookSourceFacts, HookSourceError> {
    let input = UntrustedHookSourceInput::decode(&input_bytes(event_name, event_ref))?;
    let run_body = json!({
        "id": 9001,
        "event": event_name,
        "path": ".github/workflows/ci.yml",
        "head_branch": head_branch,
        "head_sha": SHA,
        "workflow_id": 77,
        "run_attempt": 1,
        "referenced_workflows": [
            {
                "path": "owner/repo/.github/workflows/deploy.yml",
                "sha": DEPLOY_SHA,
                "ref": "refs/heads/main"
            }
        ],
        "repository": {
            "id": 7,
            "name": "repo",
            "full_name": "owner/repo",
            "owner": { "login": "owner" },
            "private": true
        }
    })
    .to_string();
    let commit_body =
        json!({"sha": SHA, "message": "c", "tree": {"sha": "2".repeat(40)}, "parents": []})
            .to_string();
    let mut script = Script::replies(vec![
        Ok(exchange(200, &run_body)),
        Ok(exchange(200, &commit_body)),
        Ok(exchange(200, &commit_body)),
    ]);
    let target = RepositoryTarget {
        id: 7,
        owner: "owner",
        name: "repo",
    };
    compare_hook_source(&mut script, &input, target, "token-1")
}

#[test]
fn event_ref_that_is_not_the_run_branch_is_refused() {
    for (event_name, event_ref, head_branch) in [
        ("workflow_dispatch", "refs/heads/main", Some("feature")),
        ("push", "refs/heads/main", Some("feature")),
        ("schedule", "refs/heads/main", Some("feature")),
        ("workflow_dispatch", "refs/heads/main", None),
        ("workflow_dispatch", "refs/heads/Feature", Some("feature")),
        (
            "merge_group",
            "refs/heads/main",
            Some("gh-readonly-queue/main/pr-1-abc"),
        ),
        ("release", "refs/tags/v1", Some("v2")),
        ("release", "refs/tags/v1", None),
        ("push", "refs/tags/v1", Some("refs/tags/v1")),
    ] {
        assert_eq!(
            compare(event_name, event_ref, head_branch),
            Err(HookSourceError::Mismatch),
            "{event_name} {event_ref} {head_branch:?}"
        );
    }
}

#[test]
fn event_ref_that_is_the_run_branch_or_tag_is_accepted() {
    for (event_name, event_ref, head_branch) in [
        ("workflow_dispatch", "refs/heads/feature", "feature"),
        ("push", "refs/heads/team/feature", "team/feature"),
        (
            "merge_group",
            "refs/heads/gh-readonly-queue/main/pr-1-abc",
            "gh-readonly-queue/main/pr-1-abc",
        ),
        ("release", "refs/tags/v1", "v1"),
    ] {
        let facts = compare(event_name, event_ref, Some(head_branch))
            .unwrap_or_else(|error| panic!("{event_name} {event_ref}: {error:?}"));
        assert_eq!(facts.event_source(), (event_name, event_ref, SHA));
    }
}

#[test]
fn pull_ref_on_a_non_pull_request_event_never_reaches_the_comparison() {
    for event_name in ["push", "workflow_dispatch", "schedule", "merge_group"] {
        assert_eq!(
            UntrustedHookSourceInput::decode(&input_bytes(event_name, "refs/pull/7/merge")),
            Err(HookSourceError::InvalidInput),
            "{event_name}"
        );
    }
}

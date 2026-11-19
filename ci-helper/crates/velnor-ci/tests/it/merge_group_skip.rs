// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_ci::github::merge_group_runs_path;
use velnor_ci::merge_group::{Context, skip_push};

use crate::support::{FakeGitHub, repo};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const WORKFLOW_REF: &str = "tailrocks/velnor-new/.github/workflows/rust-ci.yml@refs/heads/main";

fn context() -> Context {
    Context {
        event: "push".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        sha: SHA.to_owned(),
        workflow_ref: WORKFLOW_REF.to_owned(),
    }
}

fn runs(entries: &[(&str, &str, &str, &str)]) -> String {
    let runs: Vec<_> = entries
        .iter()
        .map(|(event, sha, status, conclusion)| {
            json!({"id": 1, "event": event, "head_sha": sha, "status": status, "conclusion": conclusion})
        })
        .collect();
    json!({"total_count": runs.len(), "workflow_runs": runs}).to_string()
}

fn fake(body: &str) -> FakeGitHub {
    let mut fake = FakeGitHub::default();
    fake.respond(merge_group_runs_path(&repo(), "rust-ci.yml", SHA), body);
    fake
}

#[test]
fn a_successful_merge_group_run_of_this_workflow_on_this_commit_skips() {
    let fake = fake(&runs(&[("merge_group", SHA, "completed", "success")]));
    let result = skip_push(&fake, &context());
    assert!(result.skip, "{}", result.note);
}

#[test]
fn only_a_completed_successful_merge_group_run_on_exactly_this_commit_counts() {
    let other = "f".repeat(40);
    for entry in [
        ("merge_group", SHA, "completed", "failure"),
        ("merge_group", SHA, "completed", "cancelled"),
        ("merge_group", SHA, "completed", "skipped"),
        ("merge_group", SHA, "in_progress", ""),
        ("merge_group", other.as_str(), "completed", "success"),
        ("push", SHA, "completed", "success"),
        ("pull_request", SHA, "completed", "success"),
    ] {
        let result = skip_push(&fake(&runs(&[entry])), &context());
        assert!(!result.skip, "{entry:?}");
    }
    assert!(!skip_push(&fake(&runs(&[])), &context()).skip);
}

#[test]
fn any_failure_to_find_out_runs_everything() {
    let broken = [
        FakeGitHub::default(),
        fake("not json"),
        fake(r#"{"total_count": 1}"#),
    ];
    for github in &broken {
        assert!(!skip_push(github, &context()).skip);
    }
    let mut missing = FakeGitHub::default();
    missing.missing(merge_group_runs_path(&repo(), "rust-ci.yml", SHA));
    assert!(!skip_push(&missing, &context()).skip);
}

#[test]
fn only_a_push_with_a_readable_context_asks_the_api() {
    let passed = fake(&runs(&[("merge_group", SHA, "completed", "success")]));
    for edit in [
        |c: &mut Context| c.event = "pull_request".to_owned(),
        |c: &mut Context| c.event = "merge_group".to_owned(),
        |c: &mut Context| c.event = "schedule".to_owned(),
        |c: &mut Context| c.sha = "main".to_owned(),
        |c: &mut Context| c.sha = String::new(),
        |c: &mut Context| c.repository = "not a repository".to_owned(),
        |c: &mut Context| c.workflow_ref = "rust-ci.yml".to_owned(),
        |c: &mut Context| {
            c.workflow_ref = "other/repo/.github/workflows/rust-ci.yml@refs/heads/main".to_owned();
        },
    ] {
        let mut context = context();
        edit(&mut context);
        assert!(!skip_push(&passed, &context).skip);
    }
}

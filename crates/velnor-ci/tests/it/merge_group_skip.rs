// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_ci::github::{merge_group_runs_path, run_jobs_path};
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
        required: None,
    }
}

fn with_required() -> Context {
    Context {
        required: Some("Required".to_owned()),
        ..context()
    }
}

fn jobs(entries: &[(&str, &str)]) -> String {
    let jobs: Vec<_> = entries
        .iter()
        .map(|(name, conclusion)| {
            let conclusion = (!conclusion.is_empty()).then_some(*conclusion);
            json!({"id": 9, "name": name, "conclusion": conclusion, "created_at": "2026-11-20T00:00:00Z"})
        })
        .collect();
    json!({"total_count": jobs.len(), "jobs": jobs}).to_string()
}

fn running(jobs_body: &str) -> FakeGitHub {
    let mut fake = fake(&runs(&[("merge_group", SHA, "in_progress", "")]));
    fake.respond(run_jobs_path(&repo(), 1, 1), jobs_body);
    fake
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

#[test]
fn a_queue_run_still_going_counts_once_its_required_job_has_succeeded() {
    let body = jobs(&[("Cache", ""), ("Required", "success")]);
    let result = skip_push(&running(&body), &with_required());
    assert!(result.skip, "{}", result.note);
}

#[test]
fn a_queue_run_still_going_does_not_count_without_a_succeeded_required_job() {
    let github = running(&jobs(&[("Cache", "success"), ("Required", "")]));
    assert!(!skip_push(&github, &with_required()).skip);
    let github = running(&jobs(&[("Required", "failure")]));
    assert!(!skip_push(&github, &with_required()).skip);
    let github = running(&jobs(&[("Lint", "success")]));
    assert!(!skip_push(&github, &with_required()).skip);
    let github = running(&jobs(&[("Required", "success")]));
    assert!(!skip_push(&github, &context()).skip);
    assert!(
        !skip_push(
            &fake(&runs(&[("merge_group", SHA, "in_progress", "")])),
            &with_required()
        )
        .skip
    );
}

#[test]
fn a_workflow_file_name_may_contain_an_at_sign() {
    let mut ci = FakeGitHub::default();
    ci.respond(
        merge_group_runs_path(&repo(), "ci@v2.yml", SHA),
        &runs(&[("merge_group", SHA, "completed", "success")]),
    );
    let mut context = context();
    context.workflow_ref =
        "tailrocks/velnor-new/.github/workflows/ci@v2.yml@refs/heads/main".to_owned();
    assert!(skip_push(&ci, &context).skip);
    let other = fake(&runs(&[("merge_group", SHA, "completed", "success")]));
    assert!(!skip_push(&other, &context).skip);
}

fn runs_with_ids(entries: &[(u64, &str, &str)]) -> String {
    let runs: Vec<_> = entries
        .iter()
        .map(|(id, status, conclusion)| {
            json!({"id": id, "event": "merge_group", "head_sha": SHA, "status": status, "conclusion": conclusion})
        })
        .collect();
    json!({"total_count": runs.len(), "workflow_runs": runs}).to_string()
}

#[test]
fn only_the_latest_merge_group_run_on_this_commit_decides() {
    let older_passed_newer_failed = fake(&runs_with_ids(&[
        (1, "completed", "success"),
        (2, "completed", "failure"),
    ]));
    assert!(!skip_push(&older_passed_newer_failed, &context()).skip);
    let older_failed_newer_passed = fake(&runs_with_ids(&[
        (2, "completed", "success"),
        (1, "completed", "failure"),
    ]));
    assert!(skip_push(&older_failed_newer_passed, &context()).skip);
}

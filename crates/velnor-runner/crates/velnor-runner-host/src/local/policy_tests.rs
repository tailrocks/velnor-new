// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_resource_probe::job_hook::Challenge;

use super::{Policy, Side};
use crate::launch::execution_authorization::ExecutionSourceRequest;
use crate::local::authority::RunAnswer;
use crate::local::decide::{Facts, Reason, judge};

const ROOT: &str = ".github/workflows/ci.yml";
const CONTENT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn context(event: &str, refs: &[&str], rules: Value) -> Value {
    json!({"event": event, "refs": refs, "pull_request_rules": rules})
}

fn entry(job: &str, contexts: Vec<Value>) -> Value {
    json!({
        "root_path": ROOT, "workflow_path": ROOT, "job_key": job, "role": "verification",
        "contexts": contexts, "permissions_declared": true, "permissions": {},
        "secrets": [], "environments": [], "condition_sha256": null,
        "concurrency": null, "needs": []
    })
}

fn document(calls: Value, jobs: Vec<Value>) -> Value {
    json!({
        "schema_version": 1, "connection_id": "local", "binding_sha256": CONTENT,
        "generation": 1, "policy_sha256": CONTENT,
        "generator": {"source_sha": SHA, "artifact_sha256": CONTENT},
        "workflows": [{
            "repository_id": 7, "path": ROOT, "content_sha256": CONTENT,
            "calls": calls, "jobs": jobs
        }]
    })
}

fn policy(document: &Value) -> Policy {
    Policy::parse(document.to_string().as_bytes()).expect("policy")
}

fn approved() -> Policy {
    let rule = json!([{
        "base_repository_id": 7, "base_branch": "main",
        "head_repository_id": 7, "head_branch": "topic"
    }]);
    policy(&document(
        json!([]),
        vec![entry(
            "ci",
            vec![
                context("push", &["refs/heads/main"], json!([])),
                context("pull_request", &[], rule),
            ],
        )],
    ))
}

fn request(event: &str, git_ref: &str, job: &str, pull: bool) -> ExecutionSourceRequest {
    request_from_head(event, git_ref, job, pull.then_some("topic"))
}

fn request_from(head: &str) -> ExecutionSourceRequest {
    request_from_head("pull_request", "refs/pull/2/merge", "ci", Some(head))
}

fn request_from_head(
    event: &str,
    git_ref: &str,
    job: &str,
    head: Option<&str>,
) -> ExecutionSourceRequest {
    let pull = head.is_some();
    serde_json::from_value(json!({
        "schema_version": 1, "challenge": CONTENT, "reservation_id": 1, "intent_id": 1,
        "repository_id": 7, "repository_name": "o/r", "event_name": event,
        "event_ref": git_ref, "tested_sha": SHA, "run_id": 3, "run_attempt": 1,
        "job_key": job, "workflow_ref": format!("o/r/{ROOT}@{git_ref}"),
        "workflow_sha": SHA,
        "base_ref": pull.then_some("main"), "base_sha": pull.then_some(SHA),
        "head_ref": head, "head_sha": pull.then_some(SHA),
        "pull_request_number": pull.then_some(2)
    }))
    .expect("request")
}

fn push() -> ExecutionSourceRequest {
    request("push", "refs/heads/main", "ci", false)
}

fn pull() -> ExecutionSourceRequest {
    request("pull_request", "refs/pull/2/merge", "ci", true)
}

#[test]
fn a_job_the_policy_lists_with_the_content_it_approves_passes() {
    assert!(approved().approves(&push(), Side::Trusted, Some(CONTENT)));
    assert!(approved().approves(&pull(), Side::Trusted, Some(CONTENT)));
}

#[test]
fn changed_or_unreported_workflow_content_is_refused() {
    let other = "2".repeat(64);
    assert!(!approved().approves(&push(), Side::Trusted, Some(&other)));
    assert!(!approved().approves(&push(), Side::Trusted, None));
}

#[test]
fn a_job_event_or_ref_the_policy_does_not_list_is_refused() {
    let policy = approved();
    let job = request("push", "refs/heads/main", "other", false);
    let event = request("workflow_dispatch", "refs/heads/main", "ci", false);
    let branch = request("push", "refs/heads/dev", "ci", false);
    for refused in [job, event, branch] {
        assert!(!policy.approves(&refused, Side::Trusted, Some(CONTENT)));
    }
}

#[test]
fn a_fork_pull_request_is_never_approved_and_other_branches_are_refused() {
    let policy = approved();
    assert!(!policy.approves(&pull(), Side::Untrusted, Some(CONTENT)));
    let elsewhere = request_from("elsewhere");
    assert!(!policy.approves(&elsewhere, Side::Trusted, Some(CONTENT)));
}

#[test]
fn a_workflow_that_calls_others_is_refused_because_the_called_files_are_not_hashed() {
    let call = json!([{"repository_id": 7, "path": ".github/workflows/lib.yml", "content_sha256": CONTENT}]);
    let policy = policy(&document(
        call,
        vec![entry(
            "ci",
            vec![context("push", &["refs/heads/main"], json!([]))],
        )],
    ));
    assert!(!policy.approves(&push(), Side::Trusted, Some(CONTENT)));
}

#[test]
fn a_policy_with_an_unknown_field_or_no_json_does_not_load() {
    let mut extra = document(json!([]), Vec::new());
    extra["surprise"] = json!(true);
    assert!(Policy::parse(extra.to_string().as_bytes()).is_err());
    assert!(Policy::parse(b"not json").is_err());
}

fn challenge() -> Challenge {
    serde_json::from_value(json!({
        "schema_version": 1, "challenge": CONTENT, "reservation_id": 1, "intent_id": 1,
        "witness_request_id": 1, "runner_id": 1, "runner_name": "a", "runner_scale_set_id": 1,
        "runner_container_id": "r", "dind_container_id": "d", "engine_id": "e",
        "docker_root_digest": CONTENT, "instance_binding_digest": CONTENT,
        "policy_generation": 1, "policy_digest": CONTENT, "channel_digest": CONTENT
    }))
    .expect("challenge")
}

fn answer(content: Option<&str>) -> RunAnswer {
    RunAnswer {
        id: 3,
        attempt: 1,
        event: "push".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        sha: SHA.to_owned(),
        workflow: ROOT.to_owned(),
        jobs: vec!["ci".to_owned()],
        ledger: Some("1-push".to_owned()),
        refusal: None,
        workflow_sha256: content.map(str::to_owned),
    }
}

fn facts() -> Facts {
    Facts {
        run_id: 3,
        attempt: 1,
        job: "ci".to_owned(),
        event: "push".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        sha: SHA.to_owned(),
    }
}

fn judged(policy: Option<&Policy>, content: Option<&str>) -> Option<Reason> {
    judge(
        &challenge(),
        b"{}",
        &push(),
        (Side::Trusted, policy),
        facts(),
        Ok(Some(answer(content))),
    )
    .reason
}

#[test]
fn the_decider_refuses_with_reason_policy_only_when_a_policy_is_given_and_does_not_approve() {
    let policy = approved();
    let other = "2".repeat(64);
    assert_eq!(judged(Some(&policy), Some(&other)), Some(Reason::Policy));
    assert_eq!(Reason::Policy.code(), "policy");
    assert_eq!(judged(Some(&policy), Some(CONTENT)), None);
    assert_eq!(judged(None, Some(&other)), None);
}

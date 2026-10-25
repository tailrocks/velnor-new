use super::{RunnerEnvironment, parse_runner};
use serde_json::{Value, json};
use velnor_actions_contract::WorkflowEvent;

const REPOSITORY: &str = "owner/tap";
const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MERGE: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn environment(event: &str, repository: &str, sha: &str, reference: &str) -> RunnerEnvironment {
    RunnerEnvironment {
        event_name: event.to_owned(),
        repository: repository.to_owned(),
        sha: sha.to_owned(),
        git_ref: reference.to_owned(),
    }
}

fn payload(head_repo: &str, head_id: u64, is_fork: bool) -> Value {
    json!({
        "repository": {"id": 7, "full_name": REPOSITORY},
        "pull_request": {
            "number": 12,
            "base": {
                "sha": BASE,
                "ref": "main",
                "repo": {"id": 7, "full_name": REPOSITORY}
            },
            "head": {
                "sha": HEAD,
                "ref": "topic",
                "repo": {"id": head_id, "full_name": head_repo, "fork": is_fork}
            }
        }
    })
}

fn parse(event: WorkflowEvent, environment: RunnerEnvironment, value: &Value) -> bool {
    parse_runner(event, 12, REPOSITORY, environment, value).is_ok()
}

#[test]
fn runner_payload_accepts_api_bound_head_and_merge_runner_sha_for_pr_and_fork() {
    let reference = "refs/pull/12/merge";
    assert!(parse(
        WorkflowEvent::PullRequest,
        environment("pull_request", REPOSITORY, MERGE, reference),
        &payload(REPOSITORY, 7, false),
    ));
    assert!(parse(
        WorkflowEvent::Fork,
        environment("pull_request", REPOSITORY, HEAD, reference),
        &payload("contributor/tap", 9, true),
    ));
}

#[test]
fn runner_payload_rejects_mismatched_event_repository_sha_ref_and_fork_shape() {
    let reference = "refs/pull/12/merge";
    let valid = payload(REPOSITORY, 7, false);
    for (event, repository, sha, git_ref) in [
        ("push", REPOSITORY, MERGE, reference),
        ("pull_request", "other/tap", MERGE, reference),
        ("pull_request", REPOSITORY, "not-a-sha", reference),
        ("pull_request", REPOSITORY, MERGE, "refs/pull/13/merge"),
    ] {
        assert!(!parse(
            WorkflowEvent::PullRequest,
            environment(event, repository, sha, git_ref),
            &valid,
        ));
    }
    assert!(!parse(
        WorkflowEvent::Fork,
        environment("pull_request", REPOSITORY, MERGE, reference),
        &valid,
    ));
    assert!(!parse(
        WorkflowEvent::Fork,
        environment("pull_request", REPOSITORY, MERGE, reference),
        &payload(REPOSITORY, 7, true),
    ));
}

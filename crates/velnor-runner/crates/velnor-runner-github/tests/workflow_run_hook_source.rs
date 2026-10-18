//! Focused tests for GitHub workflow source records.

use std::collections::VecDeque;

use serde_json::{Value, json};
use velnor_runner_github::workflow_run::{
    HookSourceError, RepositoryTarget, UntrustedHookSourceInput, compare_hook_source,
};
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

const OWNER: &str = "ChainArgos";
const REPO: &str = "java-monorepo";
const BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MERGE: &str = "cccccccccccccccccccccccccccccccccccccccc";
const WORKFLOW: &str = "dddddddddddddddddddddddddddddddddddddddd";
const RUN: i64 = 2468;
const ATTEMPT: i64 = 2;

struct Script(VecDeque<(String, Exchange)>);

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Get);
        let (path, exchange) = self.0.pop_front().ok_or(TransportFail::Reset)?;
        assert_eq!(request.path, path);
        Ok(exchange)
    }
}

fn exchange(value: &Value) -> Exchange {
    Exchange {
        status: 200,
        body: value.to_string().into_bytes(),
    }
}

fn input_value() -> Value {
    json!({
        "repository_id": 41,
        "repository": "ChainArgos/java-monorepo",
        "run_id": RUN,
        "run_attempt": ATTEMPT,
        "event_name": "pull_request",
        "event_ref": "refs/pull/2110/merge",
        "workflow_ref": "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/pull/2110/merge",
        "workflow_sha": WORKFLOW,
        "tested_sha": MERGE,
        "job_workflow_ref": "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/pull/2110/merge",
        "workflow_job_id": "build",
        "pull_request": {
            "number": 2110,
            "base": {"repository": {"id": 41, "name": REPO,
                    "full_name": "ChainArgos/java-monorepo"},
                "ref": "sea/integration", "sha": BASE},
            "head": {"repository": {"id": 73, "name": "fork-repo",
                    "full_name": "fork-owner/fork-repo"},
                "ref": "feature", "sha": HEAD}
        }
    })
}

fn run_json() -> Value {
    json!({
        "id": RUN,
        "event": "pull_request",
        "path": ".github/workflows/ci.yml@refs/pull/2110/merge",
        "head_branch": "feature",
        "head_sha": HEAD,
        "workflow_id": 11,
        "run_attempt": ATTEMPT,
        "repository": repository(41, REPO, OWNER),
        "head_repository": repository(73, "fork-repo", "fork-owner"),
        "pull_requests": [{
            "number": 2110,
            "head": {"ref": "feature", "sha": HEAD,
                "repo": {"id": 73, "name": "fork-repo"}},
            "base": {"ref": "sea/integration", "sha": BASE,
                "repo": {"id": 41, "name": REPO}}
        }],
        "referenced_workflows": []
    })
}

fn repository(id: i64, name: &str, owner: &str) -> Value {
    json!({"id": id, "name": name, "full_name": format!("{owner}/{name}"),
        "owner": {"login": owner}})
}

fn pull_json(merge_hint: &str) -> Value {
    json!({
        "number": 2110,
        "state": "open",
        "merged": false,
        "head": {"ref": "feature", "sha": HEAD,
            "repo": repository(73, "fork-repo", "fork-owner")},
        "base": {"ref": "sea/integration", "sha": BASE,
            "repo": repository(41, REPO, OWNER)},
        "merge_commit_sha": merge_hint
    })
}

fn commit(sha: &str, parents: &[&str]) -> Value {
    json!({"sha": sha, "message": "test", "tree": {"sha": WORKFLOW},
        "parents": parents.iter().map(|parent| json!({"sha": parent})).collect::<Vec<_>>()})
}

fn script(merge_parents: &[&str], pull: &Value) -> Script {
    Script(VecDeque::from([
        (
            format!("/repos/{OWNER}/{REPO}/actions/runs/{RUN}/attempts/{ATTEMPT}"),
            exchange(&run_json()),
        ),
        (
            format!("/repos/{OWNER}/{REPO}/git/commits/{WORKFLOW}"),
            exchange(&commit(WORKFLOW, &[])),
        ),
        (
            format!("/repos/{OWNER}/{REPO}/git/commits/{MERGE}"),
            exchange(&commit(MERGE, merge_parents)),
        ),
        (format!("/repos/{OWNER}/{REPO}/pulls/2110"), exchange(pull)),
    ]))
}

fn decode(value: &Value) -> Result<UntrustedHookSourceInput, HookSourceError> {
    UntrustedHookSourceInput::decode(value.to_string().as_bytes())
}

fn target() -> RepositoryTarget<'static> {
    RepositoryTarget {
        id: 41,
        owner: OWNER,
        name: REPO,
    }
}

#[test]
fn accepts_only_bounded_strict_hook_fields() {
    let bytes = input_value().to_string();
    assert!(UntrustedHookSourceInput::decode(bytes.as_bytes()).is_ok());
    assert_eq!(
        UntrustedHookSourceInput::decode(&vec![b' '; 65_537]),
        Err(HookSourceError::TooLarge)
    );
    let mut unknown = input_value();
    unknown["token"] = json!("secret");
    assert_eq!(decode(&unknown), Err(HookSourceError::InvalidInput));
    let duplicate = br#"{"run_id":1,"run_id":2}"#;
    assert_eq!(
        UntrustedHookSourceInput::decode(duplicate),
        Err(HookSourceError::InvalidInput)
    );
    let mut wrong_event = input_value();
    wrong_event["event_name"] = json!("push");
    assert_eq!(decode(&wrong_event), Err(HookSourceError::InvalidInput));
    let mut invalid_job = input_value();
    invalid_job["workflow_job_id"] = json!("2110-build");
    assert_eq!(decode(&invalid_job), Err(HookSourceError::InvalidInput));
    let mut wrong_ref = input_value();
    wrong_ref["event_ref"] = json!("refs/heads/main");
    assert_eq!(decode(&wrong_ref), Err(HookSourceError::InvalidInput));
    let mut missing_pull = input_value();
    missing_pull
        .as_object_mut()
        .expect("object")
        .remove("pull_request");
    assert_eq!(decode(&missing_pull), Err(HookSourceError::InvalidInput));
    let mut invalid_pull = input_value();
    invalid_pull["pull_request"]["number"] = json!(0);
    assert_eq!(decode(&invalid_pull), Err(HookSourceError::InvalidInput));
    let duplicate_pull = input_value()
        .to_string()
        .replace("\"number\":2110", "\"number\":2110,\"number\":2111");
    assert_eq!(
        UntrustedHookSourceInput::decode(duplicate_pull.as_bytes()),
        Err(HookSourceError::InvalidInput)
    );
}

#[test]
fn compares_exact_attempt_ordered_merge_parents_and_open_pr() {
    let input = decode(&input_value()).expect("valid input");
    let mut api = script(
        &[BASE, HEAD],
        &pull_json("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
    );
    let facts = compare_hook_source(&mut api, &input, target(), "token").expect("source matches");
    assert_eq!(facts.repository_id(), 41);
    assert_eq!(facts.run_identity(), (RUN, ATTEMPT));
    assert_eq!(facts.event_source().2, MERGE);
    assert_eq!(facts.workflow_source().1, WORKFLOW);
    assert_eq!(facts.job_workflow().1, None);
    assert_eq!(facts.pull_request().expect("PR facts").identity().0, 2110);
}

#[test]
fn rejects_reversed_or_incomplete_merge_parents() {
    let input = decode(&input_value()).expect("valid input");
    for parents in [
        &[HEAD, BASE][..],
        &[BASE][..],
        &[BASE, HEAD, WORKFLOW][..],
        &[BASE, WORKFLOW][..],
    ] {
        let mut api = script(parents, &pull_json(MERGE));
        assert_eq!(
            compare_hook_source(&mut api, &input, target(), "token"),
            Err(HookSourceError::Mismatch)
        );
    }
}

#[test]
fn rejects_root_workflow_path_mismatch() {
    let mut value = input_value();
    value["workflow_ref"] =
        json!("ChainArgos/java-monorepo/.github/workflows/other.yml@refs/pull/2110/merge");
    let input = decode(&value).expect("valid bounded input");
    let mut api = script(&[BASE, HEAD], &pull_json(MERGE));
    assert_eq!(
        compare_hook_source(&mut api, &input, target(), "token"),
        Err(HookSourceError::Mismatch)
    );
}

#[test]
fn rejects_merged_or_changed_pull_request() {
    let input = decode(&input_value()).expect("valid input");
    let mut merged = pull_json(MERGE);
    merged["merged"] = json!(true);
    let mut api = script(&[BASE, HEAD], &merged);
    assert_eq!(
        compare_hook_source(&mut api, &input, target(), "token"),
        Err(HookSourceError::Mismatch)
    );
    let mut changed = pull_json(MERGE);
    changed["base"]["ref"] = json!("main");
    let mut api = script(&[BASE, HEAD], &changed);
    assert_eq!(
        compare_hook_source(&mut api, &input, target(), "token"),
        Err(HookSourceError::Mismatch)
    );
}

#[test]
fn rejects_repository_name_change_in_the_immutable_run_snapshot() {
    let input = decode(&input_value()).expect("valid input");
    let mut api = script(&[BASE, HEAD], &pull_json(MERGE));
    let mut run: Value = serde_json::from_slice(&api.0[0].1.body).expect("run JSON");
    run["pull_requests"][0]["head"]["repo"]["name"] = json!("renamed-fork");
    api.0[0].1.body = run.to_string().into_bytes();
    assert_eq!(
        compare_hook_source(&mut api, &input, target(), "token"),
        Err(HookSourceError::Mismatch)
    );
}

#[test]
fn rejects_workflow_commit_response_for_a_different_sha() {
    let input = decode(&input_value()).expect("valid input");
    let mut api = script(&[BASE, HEAD], &pull_json(MERGE));
    api.0[1].1 = exchange(&commit(HEAD, &[]));
    assert_eq!(
        compare_hook_source(&mut api, &input, target(), "token"),
        Err(HookSourceError::Api(
            velnor_runner_github::workflow_run::WorkflowRunError::Malformed
        ))
    );
}

#[test]
fn rejects_unexpected_repository_scope_before_api_reads() {
    let input = decode(&input_value()).expect("valid input");
    let mut api = Script(VecDeque::new());
    let wrong = RepositoryTarget {
        id: 42,
        owner: OWNER,
        name: REPO,
    };
    assert_eq!(
        compare_hook_source(&mut api, &input, wrong, "token"),
        Err(HookSourceError::Mismatch)
    );
}

#[test]
fn accepts_non_pr_events_without_pr_facts() {
    for (event, event_ref, head_branch) in [
        ("push", "refs/heads/sea/integration", "sea/integration"),
        (
            "workflow_dispatch",
            "refs/heads/sea/integration",
            "sea/integration",
        ),
        ("schedule", "refs/heads/sea/integration", "sea/integration"),
        ("workflow_dispatch", "refs/tags/v1.2.3", "v1.2.3"),
        (
            "merge_group",
            "refs/heads/gh-readonly-queue/sea/integration/pr-2110",
            "gh-readonly-queue/sea/integration/pr-2110",
        ),
        ("release", "refs/tags/v1.2.3", "v1.2.3"),
    ] {
        let input = json!({
            "repository_id": 41,
            "repository": "ChainArgos/java-monorepo",
            "run_id": RUN,
            "run_attempt": ATTEMPT,
            "event_name": event,
            "event_ref": event_ref,
            "workflow_ref": format!("ChainArgos/java-monorepo/.github/workflows/ci.yml@{event_ref}"),
            "workflow_sha": WORKFLOW,
            "tested_sha": HEAD,
            "job_workflow_ref": format!("ChainArgos/java-monorepo/.github/workflows/ci.yml@{event_ref}"),
            "workflow_job_id": "build"
        });
        let input = decode(&input).expect("branch event input");
        let run = json!({
            "id": RUN, "event": event,
            "path": format!(".github/workflows/ci.yml@{event_ref}"),
            "head_branch": head_branch, "head_sha": HEAD,
            "workflow_id": 11, "run_attempt": ATTEMPT,
            "repository": repository(41, REPO, OWNER), "pull_requests": [],
            "referenced_workflows": []
        });
        let mut api = Script(VecDeque::from([
            (
                format!("/repos/{OWNER}/{REPO}/actions/runs/{RUN}/attempts/{ATTEMPT}"),
                exchange(&run),
            ),
            (
                format!("/repos/{OWNER}/{REPO}/git/commits/{WORKFLOW}"),
                exchange(&commit(WORKFLOW, &[])),
            ),
            (
                format!("/repos/{OWNER}/{REPO}/git/commits/{HEAD}"),
                exchange(&commit(HEAD, &[])),
            ),
        ]));
        let facts =
            compare_hook_source(&mut api, &input, target(), "token").expect("branch source");
        assert_eq!(facts.event_source(), (event, event_ref, HEAD));
        assert!(facts.pull_request().is_none());
    }
}

#[test]
fn rejects_unknown_mutable_reusable_source() {
    let mut value = input_value();
    value["job_workflow_ref"] = json!("other/repo/.github/workflows/reuse.yml@refs/heads/main");
    let input = decode(&value).expect("bounded input");
    let mut api = script(&[BASE, HEAD], &pull_json(MERGE));
    assert_eq!(
        compare_hook_source(&mut api, &input, target(), "token"),
        Err(HookSourceError::Mismatch)
    );
}

#[test]
fn keeps_same_path_reusable_workflow_sha_separate_from_root_source() {
    let mut value = input_value();
    value["job_workflow_ref"] =
        json!("ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/reusable");
    let input = decode(&value).expect("bounded input");
    let mut api = script(&[BASE, HEAD], &pull_json(MERGE));
    let mut run: Value = serde_json::from_slice(&api.0[0].1.body).expect("run JSON");
    run["referenced_workflows"] = json!([{
        "path": "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/reusable",
        "sha": HEAD,
        "ref": "refs/heads/reusable"
    }]);
    api.0[0].1.body = run.to_string().into_bytes();
    let facts = compare_hook_source(&mut api, &input, target(), "token")
        .expect("called workflow source matches");
    assert_eq!(facts.job_workflow().1, Some(HEAD));
}

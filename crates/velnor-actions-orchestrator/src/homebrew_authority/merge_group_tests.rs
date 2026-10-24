use super::{RunnerContext, parse};
use serde_json::{Value, json};

const REPOSITORY: &str = "jackin-project/homebrew-tap";
const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BRANCH: &str = "gh-readonly-queue/main/pr-1180-0000000000";

fn event() -> Value {
    json!({
        "action": "checks_requested",
        "repository": {"id": 42, "full_name": REPOSITORY},
        "merge_group": {
            "base_ref": "refs/heads/main",
            "base_sha": BASE,
            "head_ref": format!("refs/heads/{BRANCH}"),
            "head_sha": HEAD
        }
    })
}

fn runner() -> RunnerContext {
    RunnerContext {
        repository: Some(REPOSITORY.to_owned()),
        sha: Some(HEAD.to_owned()),
        git_ref: Some(format!("refs/heads/{BRANCH}")),
        run_id: Some("901".to_owned()),
        run_attempt: Some("2".to_owned()),
    }
}

fn parse_event(payload: &Value, runner: &RunnerContext) -> super::MergeGroupSelection {
    parse(REPOSITORY, 42, HEAD, "merge_group", runner, payload).expect("valid merge-group context")
}

#[test]
fn captures_exact_event_and_normalizes_only_the_head_branch_prefix() {
    let runner = runner();
    let selected = parse_event(&event(), &runner);
    assert_eq!(selected.repository(), REPOSITORY);
    assert_eq!(selected.repository_id(), 42);
    assert_eq!(selected.base_sha(), BASE);
    assert_eq!(selected.head_sha(), HEAD);
    assert_eq!(selected.head_ref(), format!("refs/heads/{BRANCH}"));
    assert_eq!(selected.head_branch(), BRANCH);
    assert_eq!(selected.run_id(), 901);
    assert_eq!(selected.run_attempt(), 2);
}

#[test]
fn rejects_mismatched_runner_and_payload_identity() {
    let mut cases = Vec::new();
    let mut wrong_action = event();
    wrong_action["action"] = json!("requested");
    cases.push((wrong_action, runner()));

    let mut wrong_base_ref = event();
    wrong_base_ref["merge_group"]["base_ref"] = json!("refs/heads/trunk");
    cases.push((wrong_base_ref, runner()));

    let mut wrong_repository = event();
    wrong_repository["repository"]["id"] = json!(43);
    cases.push((wrong_repository, runner()));

    let mut wrong_head = runner();
    wrong_head.sha = Some(BASE.to_owned());
    cases.push((event(), wrong_head));

    let mut wrong_ref = runner();
    wrong_ref.git_ref = Some(BRANCH.to_owned());
    cases.push((event(), wrong_ref));

    let mut zero_attempt = runner();
    zero_attempt.run_attempt = Some("0".to_owned());
    cases.push((event(), zero_attempt));

    let mut malformed_sha = event();
    malformed_sha["merge_group"]["head_sha"] = json!(HEAD.to_uppercase());
    cases.push((malformed_sha, runner()));

    for (payload, runner) in cases {
        assert!(parse(REPOSITORY, 42, HEAD, "merge_group", &runner, &payload).is_err());
    }
}

#[test]
fn rejects_non_merge_group_event_and_different_planned_head() {
    let runner = runner();
    assert!(parse(REPOSITORY, 42, HEAD, "pull_request", &runner, &event()).is_err());
    assert!(parse(REPOSITORY, 42, BASE, "merge_group", &runner, &event()).is_err());
}

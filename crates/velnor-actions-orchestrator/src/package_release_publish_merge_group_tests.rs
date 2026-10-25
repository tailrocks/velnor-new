use super::*;
use serde_json::{Value, json};

const REPOSITORY: &str = "jackin-project/homebrew-tap";
const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BRANCH: &str = "gh-readonly-queue/main/pr-1180-0000000000";
const REPOSITORY_ID: u64 = 42;
const RUN_ID: u64 = 901;
const ATTEMPT: u32 = 2;
const WORKFLOW_ID: u64 = 17;

fn selection() -> MergeGroupSelection {
    MergeGroupSelection::new(crate::package_release_publish::MergeGroupFacts {
        repository: REPOSITORY.to_owned(),
        repository_id: REPOSITORY_ID,
        base_sha: BASE.to_owned(),
        head_sha: HEAD.to_owned(),
        head_ref: format!("refs/heads/{BRANCH}"),
        head_branch: BRANCH.to_owned(),
        run_id: RUN_ID,
        run_attempt: ATTEMPT,
    })
}

fn run() -> Value {
    json!({
        "id": RUN_ID,
        "run_attempt": ATTEMPT,
        "workflow_id": WORKFLOW_ID,
        "path": CI_WORKFLOW_PATH,
        "head_sha": HEAD,
        "head_branch": BRANCH,
        "event": "merge_group",
        "status": "in_progress",
        "repository": {"id": REPOSITORY_ID, "full_name": REPOSITORY},
        "head_repository": {"id": REPOSITORY_ID, "full_name": REPOSITORY}
    })
}

fn responses() -> Vec<(String, Value)> {
    let repo_route = format!("repos/{REPOSITORY}");
    let base_compare = format!("{repo_route}/compare/{BASE}...{HEAD}");
    vec![
        (
            repo_route.clone(),
            json!({"id":REPOSITORY_ID,"full_name":REPOSITORY,"default_branch":"main"}),
        ),
        (
            format!("{repo_route}/actions/workflows/ci.yml"),
            json!({"id":WORKFLOW_ID,"name":WORKFLOW_DISPLAY_NAME,"path":CI_WORKFLOW_PATH,"state":"active"}),
        ),
        (
            format!("{repo_route}/actions/runs/{RUN_ID}/attempts/{ATTEMPT}"),
            run(),
        ),
        (
            format!(
                "{repo_route}/git/matching-refs/heads/{}",
                github::encode_segment(BRANCH)
            ),
            json!([{"ref":format!("refs/heads/{BRANCH}"),"object":{"sha":HEAD,"type":"commit"}}]),
        ),
        (
            repo_route.clone(),
            json!({"id":REPOSITORY_ID,"full_name":REPOSITORY,"default_branch":"main"}),
        ),
        (
            format!("{repo_route}/branches/main"),
            json!({"name":"main","protected":true,"commit":{"sha":BASE}}),
        ),
        (format!("{repo_route}/commits/{HEAD}"), json!({"sha":HEAD})),
        (
            format!("{base_compare}?per_page=1&page=2"),
            json!({"base_commit":{"sha":BASE},"merge_base_commit":{"sha":BASE},"status":"ahead","ahead_by":1,"total_commits":1}),
        ),
        (
            format!("{repo_route}/git/commits/{HEAD}"),
            json!({"sha":HEAD,"parents":[{"sha":BASE}]}),
        ),
        (
            format!("{repo_route}/branches/main"),
            json!({"name":"main","protected":true,"commit":{"sha":BASE}}),
        ),
    ]
}

#[test]
fn merge_group_authenticates_live_run_queue_ref_and_protected_base_lineage() {
    let (api, fixture) = GitHubApi::with_fixture(REPOSITORY, responses());
    verify_with_api(&api, &selection()).expect("complete live merge-group proof");
    fixture.assert_drained();
}

#[test]
fn current_run_must_match_group_attempt_event_head_and_both_repositories() {
    let mut cases = Vec::new();
    for (path, value) in [
        ("run_attempt", json!(1)),
        ("event", json!("pull_request")),
        ("head_sha", json!(BASE)),
        ("head_branch", json!("main")),
        (
            "path",
            json!(format!("{CI_WORKFLOW_PATH}@refs/heads/{BRANCH}")),
        ),
        ("path", json!(".github/workflows/other.yml")),
        ("repository.id", json!(43)),
        ("head_repository.full_name", json!("attacker/fork")),
    ] {
        let mut changed = run();
        if let Some((parent, leaf)) = path.split_once('.') {
            changed[parent][leaf] = value;
        } else {
            changed[path] = value;
        }
        cases.push(changed);
    }
    for observed in cases {
        let decoded: WorkflowRunResponse =
            serde_json::from_value(observed).expect("workflow-run response fixture");
        let workflow = WorkflowMetadata {
            id: WORKFLOW_ID,
            name: WORKFLOW_DISPLAY_NAME.to_owned(),
            path: CI_WORKFLOW_PATH.to_owned(),
            state: "active".to_owned(),
        };
        assert!(validate_run(&decoded, &workflow, &selection()).is_err());
    }
}

#[test]
fn queue_ref_must_be_unique_and_point_at_current_merge_group_head() {
    let mut wrong_ref = responses();
    wrong_ref[3].1 = json!([{
        "ref":format!("refs/heads/{BRANCH}"),
        "object":{"sha":BASE,"type":"commit"}
    }]);
    let (api, _) = GitHubApi::with_fixture(REPOSITORY, wrong_ref);
    assert!(verify_with_api(&api, &selection()).is_err());
}

#[test]
fn recheck_with_the_captured_selection_rejects_queue_and_main_movement() {
    let captured = selection();
    let (initial_api, _) = GitHubApi::with_fixture(REPOSITORY, responses());
    verify_with_api(&initial_api, &captured).expect("initial captured context is current");

    let mut moved_queue = responses();
    moved_queue[3].1 = json!([{
        "ref":format!("refs/heads/{BRANCH}"),
        "object":{"sha":BASE,"type":"commit"}
    }]);
    let (queue_api, _) = GitHubApi::with_fixture(REPOSITORY, moved_queue);
    assert!(verify_with_api(&queue_api, &captured).is_err());

    let mut moved_main = responses();
    moved_main[5].1["commit"]["sha"] = json!(HEAD);
    let (main_api, _) = GitHubApi::with_fixture(REPOSITORY, moved_main);
    assert!(verify_with_api(&main_api, &captured).is_err());
}

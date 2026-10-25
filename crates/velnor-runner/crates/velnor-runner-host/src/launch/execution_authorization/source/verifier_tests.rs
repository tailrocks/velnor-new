use std::collections::{BTreeMap, BTreeSet};

use super::*;
use serde_json::json;

#[test]
fn pr_root_sha_uses_authenticated_merge_commit_not_head_commit() {
    let merge = "a".repeat(40);
    let head = "b".repeat(40);
    let pr_request = request("pull_request", &merge, &merge, &head);

    assert!(root_source_commit_matches(&pr_request));
    let wrong = request("pull_request", &merge, &head, &head);
    assert!(!root_source_commit_matches(&wrong));
}

#[test]
fn push_root_sha_must_equal_authenticated_run_commit() {
    let commit = "a".repeat(40);
    let push_request = request("push", &commit, &commit, "");
    assert!(root_source_commit_matches(&push_request));

    let unrelated = "b".repeat(40);
    let wrong = request("push", &commit, &unrelated, "");
    assert!(!root_source_commit_matches(&wrong));
}

#[test]
fn approved_source_closure_rejects_a_missing_reusable_file() {
    let root = ".github/workflows/root.yml".to_owned();
    let leaf = ".github/workflows/leaf.yml".to_owned();
    let reachable = BTreeSet::from([root.clone(), leaf]);
    let source_refs = BTreeMap::from([(root, "a".repeat(40))]);

    assert!(!source_refs_match_approved_closure(
        &reachable,
        &source_refs
    ));
}

#[test]
fn approved_source_closure_accepts_all_reachable_files_once() {
    let root = ".github/workflows/root.yml".to_owned();
    let leaf = ".github/workflows/leaf.yml".to_owned();
    let reachable = BTreeSet::from([root.clone(), leaf.clone()]);
    let source_refs = BTreeMap::from([(root, "a".repeat(40)), (leaf, "b".repeat(40))]);

    assert!(source_refs_match_approved_closure(&reachable, &source_refs));
}

#[test]
fn duplicate_workflow_paths_with_distinct_refs_fail_closed() {
    let path = ".github/workflows/reusable.yml".to_owned();
    let mut source_refs = BTreeMap::new();
    assert!(insert_source_ref(&mut source_refs, path.clone(), "a".repeat(40)).is_ok());
    assert!(insert_source_ref(&mut source_refs, path, "b".repeat(40)).is_err());
}

fn request(
    event: &str,
    tested_sha: &str,
    workflow_sha: &str,
    head_sha: &str,
) -> ExecutionSourceRequest {
    serde_json::from_value(json!({
        "schema_version": 1,
        "challenge": "9".repeat(64),
        "reservation_id": 11,
        "intent_id": 12,
        "repository_id": 501,
        "repository_name": "acme/project",
        "event_name": event,
        "event_ref": if event == "pull_request" { "refs/pull/19/merge" } else { "refs/heads/main" },
        "tested_sha": tested_sha,
        "run_id": 13,
        "run_attempt": 1,
        "job_key": "verify",
        "workflow_ref": if event == "pull_request" { "acme/project/.github/workflows/build.yml@refs/pull/19/merge" } else { "acme/project/.github/workflows/build.yml@refs/heads/main" },
        "workflow_sha": workflow_sha,
        "base_ref": if event == "pull_request" { Some("main") } else { None },
        "base_sha": if event == "pull_request" { Some("c".repeat(40)) } else { None },
        "head_ref": if event == "pull_request" { Some("feature") } else { None },
        "head_sha": if event == "pull_request" { Some(head_sha) } else { None },
        "pull_request_number": if event == "pull_request" { Some(19) } else { None }
    }))
    .expect("request fixture must be valid")
}

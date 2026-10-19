//! Full PR feedback includes non-head commits without changing hashed record shape.
use super::super::super::github::ApiResponse;
use super::*;
use serde_json::json;
use std::{cell::Cell, rc::Rc};

fn context<'a>(head: &'a str, reviewers: &'a BTreeSet<u64>) -> FeedbackContext<'a> {
    FeedbackContext {
        repository: "owner/tap",
        pull_request_number: 12,
        head_sha: head,
        completion_app_id: 77,
        completion_check_name: "Review completion",
        expected_reviewer_app_ids: reviewers,
    }
}
fn api(rows: Vec<Value>, count: u64, change_head: bool) -> TapMergeGitHub {
    let reads = Rc::new(Cell::new(0));
    TapMergeGitHub::with_test_transport(move |args| {
        let route = args[0].to_str().expect("route");
        let body = if route == "repos/owner/tap/pulls/12" {
            reads.set(reads.get() + 1);
            json!({"number":12,"commits":count,"head":{"sha":if change_head && reads.get()>1 {"c".repeat(40)}else{"a".repeat(40)}}})
        } else if route == "repos/owner/tap/pulls/12/commits?per_page=100&page=1" {
            json!(rows)
        } else if route.contains("/commits/") {
            let sha = route.split('/').nth(4).expect("commit");
            json!([{"id":if sha.starts_with('b') {1}else{2},"commit_id":sha,"body":"finding"}])
        } else {
            return Err(internal("unexpected_commit_inventory_fixture"));
        };
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&body).expect("JSON"),
        })
    })
}

#[test]
fn every_pr_commit_is_captured_in_api_order_and_head_is_rechecked() {
    let head = "a".repeat(40);
    let reviewers = BTreeSet::from([1]);
    let context = context(&head, &reviewers);
    let api = api(
        vec![json!({"sha":"b".repeat(40)}), json!({"sha":head})],
        2,
        false,
    );
    let commits = inventory(&api, &context).expect("complete PR");
    assert_eq!(commits, vec!["b".repeat(40), head.clone()]);
    let rows = comments(&api, "owner/tap", &commits).expect("all commits");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], 1);
    assert_eq!(rows[1]["id"], 2);
    require_head(&api, &context, 2).expect("stable head");
}

#[test]
fn incomplete_duplicate_reordered_or_changed_head_inventory_fails_closed() {
    let head = "a".repeat(40);
    let reviewers = BTreeSet::from([1]);
    let context = context(&head, &reviewers);
    for (rows, count) in [
        (vec![json!({"sha":head})], 2),
        (vec![json!({"sha":head}), json!({"sha":head})], 2),
        (vec![json!({"sha":head}), json!({"sha":"b".repeat(40)})], 2),
        (vec![json!({"sha":"A".repeat(40)})], 1),
    ] {
        assert!(inventory(&api(rows, count, false), &context).is_err());
    }
    let changed = api(vec![json!({"sha":head})], 1, true);
    inventory(&changed, &context).expect("initial identity");
    assert!(require_head(&changed, &context, 1).is_err());
}

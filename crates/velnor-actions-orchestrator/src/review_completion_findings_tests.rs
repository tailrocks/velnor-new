//! Synthetic custody and bijection controls; not an actual provider qualification.
use super::super::super::super::github::ApiResponse;
use super::*;
use serde_json::json;

fn fixture() -> (FeedbackRecords, Value) {
    let bot = super::super::PROVIDER_BOT;
    let rest = json!({"id":7,"node_id":"comment","body":"finding","user":{"id":bot,"type":"Bot"},"created_at":"2026-10-19T01:00:00Z","updated_at":"2026-10-19T01:00:01Z","pull_request_review_id":9});
    let row = json!({"id":"comment","databaseId":7,"body":"finding","updatedAt":rest["updated_at"],"lastEditedAt":rest["updated_at"],"author":{"__typename":"Bot","databaseId":bot},"editor":{"__typename":"Bot","databaseId":bot},"pullRequestReview":{"databaseId":9}});
    let records = FeedbackRecords {
        reviews: vec![json!({"id":9})],
        opinionated_reviews: vec![],
        review_comments: vec![rest],
        issue_comments: vec![],
        commit_comments: vec![],
        threads: vec![json!({"id":"thread","isResolved":true})],
    };
    let response = json!({"data":{"node":{"id":"thread","isResolved":true,"isOutdated":true,"pullRequest":{"number":1,"repository":{"nameWithOwner":"owner/repo"}},"comments":{"nodes":[row],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}});
    (records, response)
}

fn inspect(records: &FeedbackRecords, response: Value) -> Result<Findings, OrchestratorError> {
    let api = TapMergeGitHub::with_test_transport(move |_| {
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&response).expect("fixture"),
        })
    });
    authenticate(&api, records, ("owner/repo", 1))
}

#[test]
fn resolved_provider_finding_requires_exact_live_thread_comment_and_editor_custody() {
    let (records, response) = fixture();
    let accepted = inspect(&records, response.clone()).expect("resolved provider finding");
    assert_eq!(
        accepted.latest,
        Some(time::parse("2026-10-19T01:00:01Z").expect("time"))
    );
    assert_eq!(accepted.metadata, vec![response.clone()]);
    for pointer in [
        "/data/node/isResolved",
        "/data/node/comments/nodes/0/author/databaseId",
        "/data/node/comments/nodes/0/editor/databaseId",
        "/data/node/comments/nodes/0/pullRequestReview/databaseId",
        "/data/node/pullRequest/number",
    ] {
        let mut wrong = response.clone();
        *wrong.pointer_mut(pointer).expect("field") = json!(0);
        assert!(inspect(&records, wrong).is_err(), "{pointer}");
    }
    let mut wrong = response;
    wrong["data"]["node"]["comments"]["nodes"][0]["body"] = json!("edited by someone else");
    assert!(inspect(&records, wrong).is_err());
}

#[test]
fn missing_extra_duplicate_unresolved_and_unbounded_thread_inventory_rejects() {
    let (mut records, response) = fixture();
    records.threads.clear();
    assert!(inspect(&records, response.clone()).is_err());
    let (records, _) = fixture();
    let mut wrong = response.clone();
    wrong["data"]["node"]["comments"]["nodes"] = json!([]);
    assert!(inspect(&records, wrong).is_err());
    let mut wrong = response.clone();
    let row = wrong["data"]["node"]["comments"]["nodes"][0].clone();
    wrong["data"]["node"]["comments"]["nodes"] = json!([row, row]);
    assert!(inspect(&records, wrong).is_err());
    let mut wrong = response;
    wrong["data"]["node"]["comments"]["pageInfo"] = json!({"hasNextPage":true,"endCursor":"same"});
    assert!(inspect(&records, wrong).is_err());
}

#[test]
fn thread_comments_paginate_successfully_with_the_exact_returned_cursor() {
    let (mut records, first) = fixture();
    let mut extra = records.review_comments[0].clone();
    extra["id"] = json!(8);
    extra["node_id"] = json!("second-comment");
    records.review_comments.push(extra);
    let mut second = first.clone();
    second["data"]["node"]["comments"]["nodes"][0]["databaseId"] = json!(8);
    second["data"]["node"]["comments"]["nodes"][0]["id"] = json!("second-comment");
    let mut first = first;
    first["data"]["node"]["comments"]["pageInfo"] =
        json!({"hasNextPage":true,"endCursor":"page-two"});
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&calls);
    let api = TapMergeGitHub::with_test_transport(move |args| {
        let page = observed.get();
        observed.set(page + 1);
        assert!(page < 2);
        let cursor = args.iter().any(|arg| arg == "cursor=page-two");
        assert_eq!(cursor, page == 1);
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(if page == 0 { &first } else { &second }).expect("page"),
        })
    });
    let admitted = authenticate(&api, &records, ("owner/repo", 1)).expect("two pages");
    assert_eq!(calls.get(), 2);
    assert_eq!(admitted.metadata.len(), 2);
}

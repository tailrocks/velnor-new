//! Authenticated API fixtures bind mutable commit feedback to exact completion.
use super::{CapturedFeedback, FeedbackContext, capture_feedback, paged_commit_comments};
use crate::tap_merge::github::{ApiResponse, TapMergeGitHub};
use serde_json::{Value, json};
use std::{cell::Cell, collections::BTreeSet, rc::Rc};
use velnor_actions_contract::{
    canonical_json_bytes, digest_b3,
    homebrew_review_completion::{
        TAP_REVIEW_COMPLETION_SCHEMA, TapCompletedReviewer, TapReviewCompletionPayload,
        TapReviewVerdict,
    },
};

fn comment(id: u64, body: &str) -> Value {
    json!({"id":id,"commit_id":"a".repeat(40),"body":body,
        "updated_at":"2026-10-19T00:00:00Z"})
}

fn api(comments: Vec<Value>, fail_second_page: bool) -> (TapMergeGitHub, Rc<Cell<usize>>) {
    let pages = Rc::new(Cell::new(0));
    let observed = Rc::clone(&pages);
    let api = TapMergeGitHub::with_test_transport(move |args| {
        let strings = args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>();
        let body = if strings.iter().any(|arg| arg.contains("/commits/")) {
            observed.set(observed.get() + 1);
            let route = strings
                .iter()
                .find(|arg| arg.starts_with("repos/"))
                .expect("route");
            let head = "a".repeat(40);
            let prefix = format!("repos/owner/tap/commits/{head}/comments?per_page=100&page=");
            let page = route
                .strip_prefix(prefix.as_str())
                .expect("exact authenticated head route")
                .parse::<usize>()
                .expect("page");
            if fail_second_page && page == 2 {
                return Ok(ApiResponse {
                    status: 403,
                    body: b"{}".to_vec(),
                });
            }
            let start = (page - 1) * 100;
            json!(
                comments
                    .iter()
                    .skip(start)
                    .take(100)
                    .cloned()
                    .collect::<Vec<_>>()
            )
        } else if strings
            .iter()
            .any(|arg| arg.contains("latestOpinionatedReviews"))
        {
            json!({"data":{"repository":{"pullRequest":{"latestOpinionatedReviews":{
                "nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}})
        } else if strings.iter().any(|arg| arg.contains("reviewThreads")) {
            json!({"data":{"repository":{"pullRequest":{"reviewThreads":{
                "nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}})
        } else {
            json!([])
        };
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&body).expect("response"),
        })
    });
    (api, pages)
}

fn capture(
    api: &TapMergeGitHub,
    checks: &[Value],
) -> Result<CapturedFeedback, crate::OrchestratorError> {
    let head = "a".repeat(40);
    let reviewers = BTreeSet::from([23]);
    capture_feedback(
        api,
        &FeedbackContext {
            repository: "owner/tap",
            pull_request_number: 21,
            head_sha: &head,
            completion_app_id: 24,
            completion_check_name: "Review completion",
            expected_reviewer_app_ids: &reviewers,
        },
        checks,
    )
}

fn completion(feedback: &CapturedFeedback) -> Value {
    let head = "a".repeat(40);
    let payload = TapReviewCompletionPayload {
        schema: TAP_REVIEW_COMPLETION_SCHEMA.into(),
        repository: "owner/tap".into(),
        pull_request_number: 21,
        head_sha: head.clone(),
        verdict: TapReviewVerdict::Complete,
        feedback_digest: feedback.snapshot.canonical_digest().expect("digest"),
        has_unaddressed_findings: false,
        reviewers: vec![TapCompletedReviewer {
            app_id: 23,
            head_sha: head.clone(),
            evidence_digest: digest_b3(b"independent fixture evidence"),
            complete: true,
            has_unaddressed_findings: false,
        }],
    };
    json!({"name":"Review completion","app":{"id":24},"head_sha":head,
        "status":"completed","conclusion":"success","output":{
            "text":String::from_utf8(canonical_json_bytes(&payload).expect("canonical")).expect("utf8")}})
}

#[test]
fn new_or_edited_commit_comment_invalidates_previously_matching_completion() {
    let baseline = capture(&api(vec![], false).0, &[]).expect("baseline");
    let clean_check = completion(&baseline);
    assert!(
        capture(&api(vec![], false).0, std::slice::from_ref(&clean_check))
            .expect("matching")
            .completion
            .is_some()
    );
    let original = vec![comment(1, "finding")];
    assert!(capture(&api(original.clone(), false).0, &[clean_check]).is_err());
    let reviewed = capture(&api(original.clone(), false).0, &[]).expect("reviewed finding");
    let reviewed_check = completion(&reviewed);
    assert!(
        capture(
            &api(original, false).0,
            std::slice::from_ref(&reviewed_check)
        )
        .is_ok()
    );
    let mut edited = comment(1, "changed finding");
    edited["updated_at"] = json!("2026-10-19T00:01:00Z");
    assert!(capture(&api(vec![edited], false).0, &[reviewed_check]).is_err());
}

#[test]
fn commit_comments_are_fully_paginated_and_late_page_failure_is_not_empty_feedback() {
    let comments = (1..=101)
        .map(|id| comment(id, "finding"))
        .collect::<Vec<_>>();
    let (service, pages) = api(comments.clone(), false);
    let rows = paged_commit_comments(&service, "owner/tap", &"a".repeat(40)).expect("both pages");
    assert_eq!(rows.len(), 101);
    assert_eq!(pages.get(), 2);
    assert_eq!(rows[100]["id"], 101);
    let (failed, pages) = api(comments, true);
    assert!(capture(&failed, &[]).is_err());
    assert_eq!(pages.get(), 2);
}

#[test]
fn commit_comments_cannot_replay_other_heads_or_duplicate_service_ids() {
    let mut foreign = comment(1, "foreign");
    foreign["commit_id"] = json!("b".repeat(40));
    for comments in [
        vec![foreign],
        vec![comment(1, "first"), comment(1, "duplicate")],
        vec![comment(0, "zero")],
    ] {
        assert!(capture(&api(comments, false).0, &[]).is_err());
    }
}

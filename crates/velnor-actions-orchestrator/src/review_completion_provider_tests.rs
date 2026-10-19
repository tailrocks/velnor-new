//! Synthetic protocol fixtures qualify parser boundaries, not real provider completion.
use super::super::super::github::ApiResponse;
use super::*;
use serde_json::json;

fn records() -> FeedbackRecords {
    let head = "a".repeat(40);
    let roles = json!({"code_review":{"commit_sha":head,"status":"completed","has_any_findings":false,"has_unresolved_findings":false},"security_review":{"commit_sha":head,"status":"completed","has_any_findings":false,"has_unresolved_findings":false}});
    let body = format!(
        "{SUMMARY}\n{STATE}{roles} -->\n| 📝 **Code Review** | ✅ **Completed** <relative-time datetime=\"2026-10-19T01:02:01.500Z\">done</relative-time> |\n| 🔒 **Security Review** | ✅ **Completed** <relative-time datetime=\"2026-10-19T01:02:02Z\">done</relative-time> |"
    );
    FeedbackRecords {
        reviews: vec![
            json!({"id":9,"node_id":"review-node","user":{"id":PROVIDER_BOT},"state":"COMMENTED","body":"<!-- codex-review-receipt:v1 -->\n\n\n### 💡 Codex Review\n\nSynthetic fixture","commit_id":head,"submitted_at":"2026-10-19T01:02:01Z"}),
        ],
        opinionated_reviews: vec![],
        review_comments: vec![],
        commit_comments: vec![],
        threads: vec![],
        issue_comments: vec![
            json!({"id":8,"node_id":"summary-node","body":body,"user":{"id":PROVIDER_BOT,"type":"Bot"},"performed_via_github_app":{"id":PROVIDER_APP,"owner":{"id":PROVIDER_OWNER}},"created_at":"2026-10-19T01:00:00Z","updated_at":"2026-10-19T01:02:02Z"}),
        ],
    }
}

fn api(records: &FeedbackRecords, owner: u64, editor: u64) -> TapMergeGitHub {
    let summary = records.issue_comments[0].clone();
    let reviews = records.reviews.clone();
    let comments = records.review_comments.clone();
    TapMergeGitHub::with_test_transport(move |args| {
        let args = args.iter().map(|s| s.to_string_lossy()).collect::<Vec<_>>();
        let body = if args.iter().any(|s| s == "apps/chatgpt-codex-connector") {
            json!({"id":PROVIDER_APP,"slug":"chatgpt-codex-connector","owner":{"id":owner}})
        } else if args.iter().any(|s| s.contains("... on IssueComment")) {
            json!({"data":{"node":{"databaseId":summary["id"],"body":summary["body"],"updatedAt":summary["updated_at"],"lastEditedAt":summary["updated_at"],"author":{"__typename":"Bot","databaseId":PROVIDER_BOT},"editor":{"__typename":"Bot","databaseId":editor}}}})
        } else if args
            .iter()
            .any(|s| s.contains("... on PullRequestReviewThread"))
        {
            thread_response(&comments)
        } else if args.iter().any(|s| s.contains("... on PullRequestReview")) {
            let review = reviews
                .iter()
                .find(|r| {
                    args.iter().any(|a| {
                        *a == format!("id={}", r["node_id"].as_str().expect("review node"))
                    })
                })
                .cloned()
                .unwrap_or(Value::Null);
            json!({"data":{"node":{"databaseId":review["id"],"body":review["body"],"submittedAt":review["submitted_at"],"lastEditedAt":review.get("fixture_edited_at").unwrap_or(&Value::Null),"author":{"__typename":"Bot","databaseId":PROVIDER_BOT},"editor":review.get("fixture_editor").unwrap_or(&Value::Null),"commit":{"oid":review["commit_id"]}}}})
        } else {
            return Err(internal("unexpected_provider_fixture_request"));
        };
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&body).expect("response"),
        })
    })
}

fn thread_response(comments: &[Value]) -> Value {
    let rows = comments.iter().map(|c| json!({"id":c["node_id"],"databaseId":c["id"],"body":c["body"],"updatedAt":c["updated_at"],"lastEditedAt":c.get("fixture_edited_at").unwrap_or(&Value::Null),"editor":c.get("fixture_editor").unwrap_or(&Value::Null),"author":{"__typename":"Bot","databaseId":PROVIDER_BOT},"pullRequestReview":{"databaseId":c["pull_request_review_id"]}})).collect::<Vec<_>>();
    json!({"data":{"node":{"id":"thread","isResolved":true,"isOutdated":true,"pullRequest":{"number":1,"repository":{"nameWithOwner":"owner/repo"}},"comments":{"nodes":rows,"pageInfo":{"hasNextPage":false,"endCursor":null}}}}})
}

fn decision(records: &FeedbackRecords) -> CompletionState {
    inspect(
        &api(records, PROVIDER_OWNER, PROVIDER_BOT),
        &"a".repeat(40),
        records,
        None,
        ("owner/repo", 1),
    )
    .expect("admission")
    .0
}

#[test]
fn synthetic_closed_both_role_completion_requires_actual_exact_head_review_identity() {
    let mut fixture = records();
    assert_eq!(decision(&fixture), CompletionState::Ready);
    fixture.reviews.clear();
    assert_eq!(decision(&fixture), CompletionState::Ready);
    fixture = records();
    fixture.reviews[0]["commit_id"] = json!("b".repeat(40));
    assert_eq!(decision(&fixture), CompletionState::Ready);
    fixture = records();
    fixture.reviews[0]["id"] = json!(0);
    assert!(
        inspect(
            &api(&fixture, PROVIDER_OWNER, PROVIDER_BOT),
            &"a".repeat(40),
            &fixture,
            None,
            ("owner/repo", 1),
        )
        .is_err()
    );
}

#[test]
fn owner_author_editor_and_duplicate_summary_cannot_substitute_provider_authority() {
    let mut fixture = records();
    assert!(
        inspect(
            &api(&fixture, 1, PROVIDER_BOT),
            &"a".repeat(40),
            &fixture,
            None,
            ("owner/repo", 1),
        )
        .is_err()
    );
    assert!(
        inspect(
            &api(&fixture, PROVIDER_OWNER, 1),
            &"a".repeat(40),
            &fixture,
            None,
            ("owner/repo", 1),
        )
        .is_err()
    );
    fixture.issue_comments[0]["user"]["id"] = json!(1);
    assert!(
        inspect(
            &api(&fixture, PROVIDER_OWNER, PROVIDER_BOT),
            &"a".repeat(40),
            &fixture,
            None,
            ("owner/repo", 1),
        )
        .is_err()
    );
    fixture = records();
    fixture
        .issue_comments
        .push(fixture.issue_comments[0].clone());
    assert_eq!(decision(&fixture), CompletionState::Pending);
}

#[test]
fn fresh_restart_waits_for_each_role_and_unknown_feedback_never_implies_disposition() {
    for body in ["@codex review", "@codex security review"] {
        let mut fixture = records();
        fixture
            .issue_comments
            .push(json!({"id":10,"body":body,"updated_at":"2026-10-19T01:02:03Z"}));
        assert_eq!(decision(&fixture), CompletionState::Pending);
    }
    for body in ["fixed", "approved", "@codex review\nanything else"] {
        let mut fixture = records();
        fixture.issue_comments.push(json!({"id":10,"body":body}));
        assert_eq!(decision(&fixture), CompletionState::Blocked);
    }
    let mut fixture = records();
    fixture
        .commit_comments
        .push(json!({"id":11,"body":"finding"}));
    assert_eq!(decision(&fixture), CompletionState::Blocked);
}

#[test]
fn resolved_authenticated_provider_findings_need_new_clean_terminal_results() {
    let mut fixture = historical_finding();
    assert_eq!(decision(&fixture), CompletionState::Ready);
    fixture.review_comments[0]["created_at"] = json!("2026-10-19T01:02:03Z");
    fixture.review_comments[0]["updated_at"] = json!("2026-10-19T01:02:03Z");
    assert_eq!(decision(&fixture), CompletionState::Pending);
    fixture.review_comments[0]["user"]["id"] = json!(1);
    assert!(
        inspect(
            &api(&fixture, PROVIDER_OWNER, PROVIDER_BOT),
            &"a".repeat(40),
            &fixture,
            None,
            ("owner/repo", 1)
        )
        .is_err()
    );
}

fn historical_finding() -> FeedbackRecords {
    let mut fixture = records();
    let mut historical = fixture.reviews[0].clone();
    historical["id"] = json!(6);
    historical["node_id"] = json!("historical-review");
    historical["commit_id"] = json!("b".repeat(40));
    historical["submitted_at"] = json!("2026-10-19T01:00:00Z");
    fixture.reviews.insert(0, historical);
    fixture
        .threads
        .push(json!({"id":"thread","isResolved":true}));
    fixture.review_comments.push(json!({"id":7,"node_id":"finding","body":"actual historical finding","user":{"id":PROVIDER_BOT,"type":"Bot"},"pull_request_review_id":6,"created_at":"2026-10-19T01:00:00Z","updated_at":"2026-10-19T01:00:00Z"}));
    fixture
}

#[test]
fn historical_findings_cannot_recover_with_stale_summary_or_unknown_custody() {
    let mut fixture = historical_finding();
    // A historical receipt is custody evidence; the current role clock must
    // follow the finding even when no new formal review is emitted.
    fixture.reviews.remove(1);
    fixture.issue_comments[0]["body"] = json!(
        fixture.issue_comments[0]["body"]
            .as_str()
            .expect("body")
            .replace("2026-10-19T01:02:01.500Z", "2026-10-19T00:59:59Z")
    );
    assert_eq!(decision(&fixture), CompletionState::Pending);
    let mut fixture = historical_finding();
    let stale_security = fixture.issue_comments[0]["body"]
        .as_str()
        .expect("body")
        .replace("2026-10-19T01:02:02Z", "2026-10-19T00:59:59Z");
    fixture.issue_comments[0]["body"] = json!(stale_security);
    assert_eq!(decision(&fixture), CompletionState::Pending);
    for pointer in [
        "/review_comments/0/user/id",
        "/review_comments/0/pull_request_review_id",
    ] {
        let mut fixture = historical_finding();
        let mut value = serde_json::to_value(&fixture).expect("fixture");
        *value.pointer_mut(pointer).expect("field") = json!(1);
        fixture.review_comments = value["review_comments"].as_array().expect("rows").clone();
        assert!(
            inspect(
                &api(&fixture, PROVIDER_OWNER, PROVIDER_BOT),
                &"a".repeat(40),
                &fixture,
                None,
                ("owner/repo", 1)
            )
            .is_err()
        );
    }
    let mut fixture = historical_finding();
    // Fixture-only fields control the independent GraphQL editor response.
    fixture.review_comments[0]["fixture_edited_at"] = json!("2026-10-19T01:00:00Z");
    fixture.review_comments[0]["fixture_editor"] = json!({"__typename":"User","databaseId":1});
    assert!(
        inspect(
            &api(&fixture, PROVIDER_OWNER, PROVIDER_BOT),
            &"a".repeat(40),
            &fixture,
            None,
            ("owner/repo", 1)
        )
        .is_err()
    );
}

#[test]
fn security_receipts_cannot_substitute_for_current_code_role_completion() {
    let mut fixture = historical_finding();
    let security = fixture.reviews[1]["body"]
        .as_str()
        .expect("body")
        .replace("### 💡 Codex Review", "### 🛡️ Codex Security Review");
    fixture.reviews[1]["body"] = json!(security);
    assert_eq!(decision(&fixture), CompletionState::Ready);
    let clean_summary = fixture.issue_comments[0]["body"].clone();
    fixture.issue_comments[0]["body"] = json!(clean_summary.as_str().expect("body").replacen(
        "\"status\":\"completed\"",
        "\"status\":\"running\"",
        1
    ));
    assert_eq!(decision(&fixture), CompletionState::Pending);
    fixture.issue_comments[0]["body"] = clean_summary;
    // Historical security receipts remain authenticated evidence; they cannot
    // substitute for actual completed code-role authority in the summary.
    fixture.reviews.swap(0, 1);
    fixture.reviews[1]["body"] = fixture.reviews[0]["body"].clone();
    fixture.reviews[0]["body"] = records().reviews[0]["body"].clone();
    assert_eq!(decision(&fixture), CompletionState::Ready);
    fixture.reviews[0]["body"] = json!("<!-- codex-review-receipt:v1 -->\n### Unknown Review");
    assert!(
        inspect(
            &api(&fixture, PROVIDER_OWNER, PROVIDER_BOT),
            &"a".repeat(40),
            &fixture,
            None,
            ("owner/repo", 1)
        )
        .is_err()
    );
}

#[test]
fn missing_stale_null_running_findings_and_unknown_role_states_are_not_clean() {
    for replacement in ["null", "true"] {
        let mut fixture = records();
        let body = fixture.issue_comments[0]["body"]
            .as_str()
            .expect("body")
            .replace(
                "\"has_any_findings\":false",
                &format!("\"has_any_findings\":{replacement}"),
            );
        fixture.issue_comments[0]["body"] = json!(body);
        assert_eq!(decision(&fixture), CompletionState::Pending);
    }
    for status in ["running", "failed", "invented"] {
        let mut fixture = records();
        let body = fixture.issue_comments[0]["body"]
            .as_str()
            .expect("body")
            .replace(
                "\"status\":\"completed\"",
                &format!("\"status\":\"{status}\""),
            );
        fixture.issue_comments[0]["body"] = json!(body);
        let result = inspect(
            &api(&fixture, PROVIDER_OWNER, PROVIDER_BOT),
            &"a".repeat(40),
            &fixture,
            None,
            ("owner/repo", 1),
        );
        assert!(result.is_err() || result.expect("known state").0 != CompletionState::Ready);
    }
    let fixture = records();
    assert_ne!(
        inspect(
            &api(&fixture, PROVIDER_OWNER, PROVIDER_BOT),
            &"b".repeat(40),
            &fixture,
            None,
            ("owner/repo", 1),
        )
        .expect("stale")
        .0,
        CompletionState::Ready
    );
}

#[test]
fn provider_marker_rejects_duplicate_keys_unknown_fields_and_ambiguous_markers() {
    for body in [
        format!("{STATE}{{\"code_review\":null,\"code_review\":null}} -->"),
        format!("{STATE}{{\"unknown\":true}} -->"),
        format!("{STATE}{{}} -->\n{STATE}{{}} -->"),
    ] {
        assert!(parse_state(&body).is_err());
    }
}

#[test]
fn strict_role_clocks_reject_invalid_dates_and_preserve_fractional_order() {
    assert!(time::parse("2024-02-29T00:00:00Z").is_ok());
    for bad in [
        "2026-02-29T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-10-19T25:00:00Z",
        "2026-10-19T00:00:60Z",
        "é026-10-19T00:00:00Z",
        "2026-10-19T00:00:00+00:00",
    ] {
        assert!(time::parse(bad).is_err(), "{bad}");
    }
    assert!(
        time::parse("2026-10-19T00:00:00.001Z").expect("fraction")
            > time::parse("2026-10-19T00:00:00Z").expect("seconds")
    );
}

#[test]
fn deleted_current_event_trigger_still_invalidates_an_old_same_head_completion() {
    let fixture = records();
    for role in ["code", "security"] {
        let result = inspect(
            &api(&fixture, PROVIDER_OWNER, PROVIDER_BOT),
            &"a".repeat(40),
            &fixture,
            Some((role, "2026-10-19T01:02:03Z")),
            ("owner/repo", 1),
        )
        .expect("event restart");
        assert_eq!(result.0, CompletionState::Pending);
    }
}

#[path = "review_completion_provider_clean_tests.rs"]
mod clean_tests;

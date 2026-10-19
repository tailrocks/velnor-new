//! Retained public PR177 wire data proves the genuine no-formal-review protocol.
use super::*;

fn captured() -> (FeedbackRecords, Value) {
    let wire: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/codex-pr177-clean-summary.json"
    ))
    .expect("retained authenticated public capture");
    assert_eq!(wire["provenance"]["pull_request"], 177);
    assert_eq!(wire["comments"][0]["id"], 6_265_103_425_u64);
    assert_eq!(wire["reviews"], json!([]));
    assert_eq!(wire["inline_pages"], json!([[]]));
    let mut fixture = records();
    fixture.reviews.clear();
    fixture.issue_comments = wire["comments"].as_array().expect("comments").clone();
    (fixture, wire)
}

fn captured_decision(fixture: &FeedbackRecords) -> Result<CompletionState, OrchestratorError> {
    inspect(
        &api(fixture, PROVIDER_OWNER, PROVIDER_BOT),
        "2bc7dc02b508d99dcf97b2afccef833a5d54d7af",
        fixture,
        None,
        ("tailrocks/velnor-new", 177),
    )
    .map(|result| result.0)
}

#[test]
fn genuine_clean_summary_without_formal_reviews_is_ready_without_reaction_authority() {
    let (fixture, wire) = captured();
    let summary = fixture.issue_comments[0].clone();
    let editor = wire["editor"].clone();
    let transport = TapMergeGitHub::with_test_transport(move |args| {
        let args = args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>();
        let value = if args.iter().any(|a| a == "apps/chatgpt-codex-connector") {
            summary["performed_via_github_app"].clone()
        } else if args.iter().any(|a| a.contains("... on IssueComment")) {
            editor.clone()
        } else {
            return Err(internal("unexpected_clean_capture_request"));
        };
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&value).expect("capture response"),
        })
    });
    assert_eq!(
        inspect(
            &transport,
            wire["provenance"]["head"].as_str().expect("head"),
            &fixture,
            None,
            ("tailrocks/velnor-new", 177),
        )
        .expect("authenticated original wire")
        .0,
        CompletionState::Ready
    );
}

#[test]
fn receipt_free_summary_still_requires_both_exact_head_clean_roles_and_fresh_clocks() {
    let (fixture, _) = captured();
    let body = fixture.issue_comments[0]["body"].as_str().expect("body");
    for (from, to) in [
        ("\"status\":\"completed\"", "\"status\":\"running\""),
        ("\"has_any_findings\":false", "\"has_any_findings\":true"),
        (
            "\"has_unresolved_findings\":false",
            "\"has_unresolved_findings\":true",
        ),
        ("\"has_any_findings\":false", "\"has_any_findings\":null"),
        ("\"has_unresolved_findings\":false,", ""),
        ("\"security_review\":", "\"missing_security_review\":"),
        (
            "2bc7dc02b508d99dcf97b2afccef833a5d54d7af",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        ("2026-10-19T10:29:51.022549Z", "not-a-date"),
    ] {
        assert!(body.contains(from), "nonvacuous mutation {from}");
        let mut changed = captured().0;
        changed.issue_comments[0]["body"] = json!(body.replacen(from, to, 1));
        assert!(!captured_decision(&changed).is_ok_and(|s| s == CompletionState::Ready));
    }
    for role in ["code", "security"] {
        assert_eq!(
            inspect(
                &api(&fixture, PROVIDER_OWNER, PROVIDER_BOT),
                "2bc7dc02b508d99dcf97b2afccef833a5d54d7af",
                &fixture,
                Some((role, "2026-10-19T10:32:00Z")),
                ("tailrocks/velnor-new", 177),
            )
            .expect("restart")
            .0,
            CompletionState::Pending
        );
    }
}

#[test]
fn receipt_free_positive_cannot_hide_forged_editor_unknown_feedback_or_missing_summary() {
    let (fixture, _) = captured();
    assert!(
        inspect(
            &api(&fixture, PROVIDER_OWNER, 1),
            "2bc7dc02b508d99dcf97b2afccef833a5d54d7af",
            &fixture,
            None,
            ("tailrocks/velnor-new", 177),
        )
        .is_err()
    );
    let mut changed = captured().0;
    changed
        .issue_comments
        .push(json!({"id":123,"body":"approved"}));
    assert_eq!(
        captured_decision(&changed).expect("unknown feedback"),
        CompletionState::Blocked
    );
    changed = captured().0;
    changed.issue_comments[0]["body"] = json!("Codex approved. 👍");
    assert_eq!(
        captured_decision(&changed).expect("missing summary"),
        CompletionState::Pending
    );
    changed = fixture;
    changed
        .commit_comments
        .push(json!({"id":124,"body":"finding"}));
    assert_eq!(
        captured_decision(&changed).expect("commit feedback"),
        CompletionState::Blocked
    );
}

#[test]
fn resolved_findings_can_receive_fresh_clean_summary_without_new_formal_review() {
    let mut fixture = historical_finding();
    fixture.reviews.remove(1);
    assert_eq!(decision(&fixture), CompletionState::Ready);
    fixture.review_comments[0]["updated_at"] = json!("2026-10-19T01:02:03Z");
    fixture.review_comments[0]["fixture_edited_at"] = json!("2026-10-19T01:02:03Z");
    fixture.review_comments[0]["fixture_editor"] =
        json!({"__typename":"Bot","databaseId":PROVIDER_BOT});
    assert_eq!(decision(&fixture), CompletionState::Pending);
}

#[test]
fn present_receipts_wait_for_matching_role_summary_to_cover_submission_and_edit() {
    for (heading, covered) in [
        ("### 💡 Codex Review", "2026-10-19T01:02:01.500Z"),
        ("### 🛡️ Codex Security Review", "2026-10-19T01:02:02Z"),
    ] {
        let mut fixture = records();
        fixture.reviews[0]["body"] = json!(format!("<!-- codex-review-receipt:v1 -->\n{heading}"));
        // Between the two terminal clocks: Code must wait, Security is covered.
        fixture.reviews[0]["submitted_at"] = json!("2026-10-19T01:02:01.750Z");
        assert_eq!(
            decision(&fixture),
            if heading.contains("Security") {
                CompletionState::Ready
            } else {
                CompletionState::Pending
            }
        );
        fixture.reviews[0]["submitted_at"] = json!(covered);
        assert_eq!(decision(&fixture), CompletionState::Ready);
        fixture.reviews[0]["fixture_edited_at"] = json!("2026-10-19T01:02:03Z");
        fixture.reviews[0]["fixture_editor"] =
            json!({"__typename":"Bot","databaseId":PROVIDER_BOT});
        assert_eq!(decision(&fixture), CompletionState::Pending);
        fixture.reviews[0]["fixture_edited_at"] = json!(covered);
        assert_eq!(decision(&fixture), CompletionState::Ready);
    }
}

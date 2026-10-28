// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{
    Fixture, Scenario, assert_completion, assert_success, assert_transport_clean, read_json,
};
use super::{Case, LATE_TIME, USER_ID, capture, run, write_case};
use serde_json::json;
use std::{error::Error, fs};

#[test]
fn resolved_human_replies_in_comment_only_reviews_complete() -> Result<(), Box<dyn Error>> {
    run(Case::BotFollowup, true, "ready")?;
    run(Case::OneReply, true, "ready")?;
    run(Case::MultipleReplies, true, "ready")?;
    run(Case::HistoricalCommit, true, "ready")
}

#[test]
fn malformed_reply_and_review_relationships_never_complete() -> Result<(), Box<dyn Error>> {
    for case in [
        Case::WrongParent,
        Case::ForeignBotFollowupParent,
        Case::MissingParent,
        Case::ForeignRepository,
        Case::ForeignPullRequest,
        Case::OrphanReview,
        Case::NonemptyReview,
        Case::MismatchedAuthor,
        Case::MismatchedEditor,
        Case::MismatchedBody,
        Case::MismatchedTime,
        Case::MismatchedId,
        Case::UnresolvedThread,
        Case::ChangeRequest,
    ] {
        run(case, false, "blocked")?;
    }
    Ok(())
}

#[test]
fn late_reply_activity_waits_for_both_exact_head_roles() -> Result<(), Box<dyn Error>> {
    run(Case::LateActivity, false, "pending")?;
    run(Case::BothRolesLater, true, "ready")
}

#[test]
fn review_restart_after_capture_prevents_success() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(Scenario::InlineFullDatabaseId)?;
    write_case(&fixture.state, Case::OneReply)?;
    let (proposal, digest) = capture(&fixture)?;
    assert_eq!(proposal["state"], "ready");

    let mut issue_comments = read_json(&fixture.state.join("issue-comments.json"))?;
    issue_comments
        .as_array_mut()
        .ok_or("issue comments not an array")?
        .push(json!({
            "id":6_402_121_075_u64,"node_id":"IC_late_review_request",
            "user":{"id":USER_ID,"type":"User"},
            "created_at":LATE_TIME,"updated_at":LATE_TIME,"body":"@codex review"
        }));
    fs::write(
        fixture.state.join("issue-comments-after-capture.json"),
        serde_json::to_vec(&issue_comments)?,
    )?;
    fs::write(fixture.state.join("mutate-on-check-post"), b"1")?;

    let emitted = fixture
        .command("emit-review-completion-v1")
        .env("VELNOR_REVIEW_COMPLETION_PROPOSAL_DIGEST", digest)
        .output()?;
    assert_transport_clean(&fixture.state)?;
    assert_success(&emitted, "emit")?;
    let check = read_json(&fixture.state.join("completion-check.json"))?;
    assert_completion(&check, false)?;
    assert!(!fixture.state.join("mutate-on-check-post").exists());
    Ok(())
}

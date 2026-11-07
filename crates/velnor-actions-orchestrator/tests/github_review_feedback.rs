// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    FeedbackContext, FeedbackQuery, FeedbackRead, capture_feedback, verify_complete_feedback,
};
use crate::{OrchestratorError, internal::internal};
use serde_json::{Value, json};
use std::{cell::Cell, collections::BTreeSet};
use velnor_actions_contract::{
    canonical_json_bytes,
    homebrew_review_completion::{
        TAP_REVIEW_COMPLETION_SCHEMA, TapCompletedReviewer, TapReviewCompletionPayload,
        TapReviewVerdict,
    },
};

struct GitHubFixture {
    unresolved: Cell<bool>,
    mutate_head: Cell<bool>,
    duplicate_threads: Cell<bool>,
    duplicate_reviews: Cell<bool>,
    pulls: Cell<u32>,
}
impl FeedbackRead for GitHubFixture {
    fn feedback_get(&self, route: &str) -> Result<Value, OrchestratorError> {
        if route == "repos/acme/repo/pulls/7" {
            let count = self.pulls.get();
            self.pulls.set(count + 1);
            let head = if self.mutate_head.get() && count >= 2 {
                "b"
            } else {
                "a"
            }
            .repeat(40);
            return Ok(json!({"number":7,"head":{"sha":head},"commits":1}));
        }
        if route == "repos/acme/repo/pulls/7/commits?per_page=100&page=1" {
            return Ok(json!([{"sha":"a".repeat(40)}]));
        }
        if route == "repos/acme/repo/pulls/7/reviews?per_page=100&page=1" {
            if self.duplicate_reviews.get() {
                return Ok(Value::Array((1..=100).map(|id| json!({"id":id})).collect()));
            }
            return Ok(json!([{"id":3,"body":"review"}]));
        }
        if route == "repos/acme/repo/pulls/7/reviews?per_page=100&page=2"
            && self.duplicate_reviews.get()
        {
            return Ok(json!([{"id":100}]));
        }
        if route == "repos/acme/repo/pulls/7/comments?per_page=100&page=1" {
            return Ok(json!([{"id":4,"body":"inline"}]));
        }
        if route == "repos/acme/repo/issues/7/comments?per_page=100&page=1" {
            return Ok(json!([{"id":5,"body":"general"}]));
        }
        if route
            == format!(
                "repos/acme/repo/commits/{}/comments?per_page=100&page=1",
                "a".repeat(40)
            )
        {
            return Ok(json!([{"id":6,"commit_id":"a".repeat(40),"body":"commit"}]));
        }
        Err(internal("unexpected_fixture_route"))
    }
    fn feedback_graphql(
        &self,
        query: FeedbackQuery,
        fields: &[(&str, String)],
    ) -> Result<Value, OrchestratorError> {
        match query {
            FeedbackQuery::Threads => {
                let first = self.duplicate_threads.get()
                    && !fields.iter().any(|(name, _)| *name == "cursor");
                Ok(
                    json!({"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"thread","isResolved":!self.unresolved.get(),"isOutdated":true}],"pageInfo":{"hasNextPage":first,"endCursor":"cursor-1"}}}}}}),
                )
            }
            FeedbackQuery::Opinions => Ok(
                json!({"data":{"repository":{"pullRequest":{"latestOpinionatedReviews":{"nodes":[],"pageInfo":{"hasNextPage":false}}}}}}),
            ),
        }
    }
}
fn fixture() -> GitHubFixture {
    GitHubFixture {
        unresolved: Cell::new(false),
        mutate_head: Cell::new(false),
        duplicate_threads: Cell::new(false),
        duplicate_reviews: Cell::new(false),
        pulls: Cell::new(0),
    }
}
fn context<'a>(head: &'a str, reviewers: &'a BTreeSet<u64>) -> FeedbackContext<'a> {
    FeedbackContext {
        repository: "acme/repo",
        pull_request_number: 7,
        head_sha: head,
        completion_app_id: 80,
        completion_check_name: "review-complete",
        expected_reviewer_app_ids: reviewers,
    }
}
fn completion_check(
    api: &GitHubFixture,
    context: &FeedbackContext<'_>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let observed = capture_feedback(api, context, &[])?;
    let payload = TapReviewCompletionPayload {
        schema: TAP_REVIEW_COMPLETION_SCHEMA.into(),
        repository: context.repository.into(),
        pull_request_number: 7,
        head_sha: context.head_sha.into(),
        verdict: TapReviewVerdict::Complete,
        feedback_digest: observed.snapshot.canonical_digest()?,
        has_unaddressed_findings: false,
        reviewers: vec![TapCompletedReviewer {
            app_id: 90,
            head_sha: context.head_sha.into(),
            evidence_digest: format!("b3-{}", "c".repeat(64)),
            complete: true,
            has_unaddressed_findings: false,
        }],
    };
    let text = String::from_utf8(canonical_json_bytes(&payload)?)?;
    Ok(
        json!({"name":"review-complete","app":{"id":80},"head_sha":context.head_sha,"status":"completed","conclusion":"success","output":{"text":text}}),
    )
}
#[test]
fn complete_feedback_requires_matching_trusted_receipt_and_no_unresolved_threads()
-> Result<(), Box<dyn std::error::Error>> {
    let api = fixture();
    let head = "a".repeat(40);
    let reviewers = BTreeSet::from([90]);
    let ctx = context(&head, &reviewers);
    let check = completion_check(&api, &ctx)?;
    assert!(verify_complete_feedback(&api, &ctx, std::slice::from_ref(&check)).is_ok());
    assert!(verify_complete_feedback(&api, &ctx, &[]).is_err());
    api.unresolved.set(true);
    let changed_check = completion_check(&api, &ctx)?;
    assert!(verify_complete_feedback(&api, &ctx, &[changed_check]).is_err());
    Ok(())
}
#[test]
fn complete_feedback_rejects_candidate_head_changing_between_captures()
-> Result<(), Box<dyn std::error::Error>> {
    let api = fixture();
    let head = "a".repeat(40);
    let reviewers = BTreeSet::from([90]);
    let ctx = context(&head, &reviewers);
    let check = completion_check(&api, &ctx)?;
    api.pulls.set(0);
    api.mutate_head.set(true);
    assert!(verify_complete_feedback(&api, &ctx, &[check]).is_err());
    Ok(())
}

#[test]
fn feedback_rejects_duplicate_thread_across_pages() {
    let api = fixture();
    let head = "a".repeat(40);
    let reviewers = BTreeSet::from([90]);
    let ctx = context(&head, &reviewers);
    api.duplicate_threads.set(true);
    assert!(capture_feedback(&api, &ctx, &[]).is_err());
}

#[test]
fn feedback_rejects_duplicate_review_across_pages() {
    let api = fixture();
    let head = "a".repeat(40);
    let reviewers = BTreeSet::from([90]);
    let ctx = context(&head, &reviewers);
    api.duplicate_reviews.set(true);
    assert!(capture_feedback(&api, &ctx, &[]).is_err());
}

#[test]
fn refreshed_feedback_rejects_check_revocation_and_feedback_change()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_github::GitHubAdmissionRead;
    use crate::crate_release_qualification_checks::verify_refreshed_feedback;
    struct Checks {
        value: Value,
        calls: Cell<u32>,
        revoke: bool,
    }
    impl GitHubAdmissionRead for Checks {
        fn get(&self, _path: &str) -> Result<Value, OrchestratorError> {
            let n = self.calls.get();
            self.calls.set(n + 1);
            let mut check = self.value.clone();
            if self.revoke && n >= 2 {
                check["conclusion"] = "failure".into();
            }
            Ok(json!({"total_count":1,"check_runs":[check]}))
        }
        fn get_optional(&self, path: &str) -> Result<Option<Value>, OrchestratorError> {
            self.get(path).map(Some)
        }
    }
    let api = fixture();
    let head = "a".repeat(40);
    let reviewers = BTreeSet::from([90]);
    let ctx = context(&head, &reviewers);
    let mut check = completion_check(&api, &ctx)?;
    check["id"] = 123.into();
    let checks = Checks {
        value: check.clone(),
        calls: Cell::new(0),
        revoke: false,
    };
    assert!(verify_refreshed_feedback(&checks, &api, &ctx).is_ok());
    let revoked = Checks {
        value: check.clone(),
        calls: Cell::new(0),
        revoke: true,
    };
    assert!(verify_refreshed_feedback(&revoked, &api, &ctx).is_err());
    let checks = Checks {
        value: check,
        calls: Cell::new(0),
        revoke: false,
    };
    api.pulls.set(0);
    api.mutate_head.set(true);
    assert!(verify_refreshed_feedback(&checks, &api, &ctx).is_err());
    Ok(())
}

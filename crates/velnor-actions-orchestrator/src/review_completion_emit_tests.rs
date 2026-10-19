//! Synthetic Checks API fixtures exercise custody, invalidation and idempotence.
use super::super::super::github::{ApiResponse, TapMergeGitHub};
use super::*;
use serde_json::json;
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};
use velnor_actions_contract::{
    config::ReviewCompletionProvider,
    digest_b3,
    review_completion::{REVIEW_COMPLETION_POLICY_SCHEMA, ReviewCompletionPolicy},
};
use velnor_actions_mise::CheckDeadline;

fn policy() -> ReviewCompletionPolicy {
    ReviewCompletionPolicy {
        schema: REVIEW_COMPLETION_POLICY_SCHEMA.into(),
        repository: "owner/tap".into(),
        default_branch: "main".into(),
        provider: ReviewCompletionProvider::CodexV1,
        check_name: "Review completion".into(),
    }
}
fn operation(api: TapMergeGitHub) -> Operation {
    Operation {
        policy: policy(),
        temp: "/unused-in-protocol-fixture".into(),
        app_id: 77,
        repository_id: 1,
        api,
        deadline: CheckDeadline::from_start(Instant::now(), Duration::from_secs(5))
            .expect("deadline"),
    }
}
fn proposal(state: CompletionState) -> Proposal {
    Proposal {
        schema: "velnor.review-completion-proposal.v1".into(),
        policy_digest: digest_b3(b"policy"),
        repository: "owner/tap".into(),
        repository_id: 1,
        pull_request_number: 12,
        head_sha: "a".repeat(40),
        completion_app_id: 77,
        reviewer_app_id: state::PROVIDER_APP,
        feedback_digest: Some(digest_b3(b"feedback")),
        provider_digest: Some(digest_b3(b"provider")),
        state,
    }
}
fn check(conclusion: &str) -> Value {
    json!({"id":42,"name":"Review completion","app":{"id":77},"head_sha":"a".repeat(40),"status":"completed","conclusion":conclusion})
}

#[test]
fn one_check_is_created_then_updated_and_old_green_is_actually_invalidated() {
    let stored = Rc::new(RefCell::new(Value::Null));
    let observed = Rc::clone(&stored);
    let post_count = Rc::new(RefCell::new(0));
    let counter = Rc::clone(&post_count);
    let api = TapMergeGitHub::with_test_transport(move |args| {
        let args = args
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        if args.iter().any(|s| s.contains("/check-runs?")) {
            let rows = if observed.borrow().is_null() {
                vec![]
            } else {
                vec![observed.borrow().clone()]
            };
            return Ok(ApiResponse {
                status: 200,
                body: serde_json::to_vec(&json!({"check_runs":rows})).expect("list"),
            });
        }
        let post = args.iter().any(|s| s == "POST");
        assert!(post || args.iter().any(|s| s == "PATCH"));
        if post {
            *counter.borrow_mut() += 1;
            assert!(observed.borrow().is_null());
        }
        let field = |name: &str| {
            args.iter()
                .find_map(|s| s.strip_prefix(&format!("{name}=")))
                .expect("field")
                .to_owned()
        };
        let mut response = check(&field("conclusion"));
        response["output"] = json!({"text":field("output[text]")});
        *observed.borrow_mut() = response.clone();
        Ok(ApiResponse {
            status: if post { 201 } else { 200 },
            body: serde_json::to_vec(&response).expect("check"),
        })
    });
    let operation = operation(api);
    let head = "a".repeat(40);
    assert_eq!(unique_check(&operation, &head).expect("none"), None);
    write(&operation, None, &head, false, "Pending").expect("initial invalidation");
    assert_eq!(stored.borrow()["conclusion"], "failure");
    let id = unique_check(&operation, &head)
        .expect("unique")
        .expect("id");
    let text =
        completion(&proposal(CompletionState::Ready), "Review completion").expect("canonical");
    write(&operation, Some(id), &head, true, &text).expect("ready");
    assert_eq!(stored.borrow()["conclusion"], "success");
    write(&operation, Some(id), &head, false, "New review pending").expect("invalidate green");
    assert_eq!(stored.borrow()["conclusion"], "failure");
    assert_eq!(*post_count.borrow(), 1);
}

#[test]
fn pending_or_blocked_cannot_form_canonical_success_and_foreign_apps_reject() {
    for state in [CompletionState::Pending, CompletionState::Blocked] {
        assert!(completion(&proposal(state), "Review completion").is_err());
    }
    let api = TapMergeGitHub::with_test_transport(|_| {
        let mut wrong = check("failure");
        wrong["app"]["id"] = json!(78);
        Ok(ApiResponse {
            status: 201,
            body: serde_json::to_vec(&wrong).expect("wrong app"),
        })
    });
    assert!(write(&operation(api), None, &"a".repeat(40), false, "pending").is_err());
    let api = TapMergeGitHub::with_test_transport(|_| {
        Ok(ApiResponse {
            status: 403,
            body: b"{}".to_vec(),
        })
    });
    assert!(write(&operation(api), None, &"a".repeat(40), false, "pending").is_err());
}

#[test]
fn duplicate_app_name_head_checks_are_not_arbitrarily_selected() {
    let api = TapMergeGitHub::with_test_transport(|_| {
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&json!({"check_runs":[check("success"),check("failure")]}))
                .expect("duplicates"),
        })
    });
    assert!(unique_check(&operation(api), &"a".repeat(40)).is_err());
}

#[test]
fn canonical_proposal_requires_captured_digest_and_exact_policy_app_repository_binding() {
    let api = TapMergeGitHub::with_test_transport(|_| {
        Err(internal("proposal_read_must_not_use_network"))
    });
    let mut operation = operation(api);
    let temp = tempfile::tempdir().expect("temp");
    operation.temp = temp.path().canonicalize().expect("canonical temp");
    let mut proposed = proposal(CompletionState::Pending);
    proposed.policy_digest = digest_b3(&operation.policy.canonical_bytes().expect("policy"));
    let bytes = canonical_json_bytes(&proposed).expect("proposal");
    let path = operation.temp.join(state::PROPOSAL_NAME);
    std::fs::write(&path, &bytes).expect("write");
    assert_eq!(
        state::read_proposal(&operation, &digest_b3(&bytes)).expect("captured"),
        proposed
    );
    assert!(state::read_proposal(&operation, "").is_err());
    let mut altered = bytes.clone();
    altered.push(b' ');
    std::fs::write(&path, &altered).expect("tamper");
    assert!(state::read_proposal(&operation, &digest_b3(&bytes)).is_err());
    assert!(state::read_proposal(&operation, &digest_b3(&altered)).is_err());
    proposed.completion_app_id = 78;
    let bytes = canonical_json_bytes(&proposed).expect("wrong App");
    std::fs::write(&path, &bytes).expect("replace");
    assert!(state::read_proposal(&operation, &digest_b3(&bytes)).is_err());
}

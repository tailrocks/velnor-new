use super::*;

fn opinion(id: u64, user: u64, date: &str, state: &str) -> Value {
    serde_json::json!({
        "id": id,
        "author": {"databaseId": user},
        "submitted_at": date,
        "state": state
    })
}

#[test]
fn latest_decisive_actor_review_state_controls_outstanding_change_requests() {
    let reviews = vec![
        opinion(1, 10, "2026-10-02T00:00:00Z", "APPROVED"),
        opinion(3, 11, "2026-10-01T00:00:00Z", "CHANGES_REQUESTED"),
        opinion(5, 12, "2026-10-01T00:00:00Z", "CHANGES_REQUESTED"),
    ];
    assert_eq!(
        outstanding_change_requests(&reviews).expect("review states"),
        2
    );
}

#[test]
fn newer_change_request_remains_outstanding() {
    let reviews = vec![opinion(2, 10, "2026-10-02T00:00:00Z", "CHANGES_REQUESTED")];
    assert_eq!(
        outstanding_change_requests(&reviews).expect("review states"),
        1
    );
}

#[test]
fn latest_opinionated_review_keeps_prior_request_after_comment_but_approval_clears_it() {
    let after_comment = [opinion(1, 10, "2026-10-01T00:00:00Z", "CHANGES_REQUESTED")];
    assert_eq!(
        outstanding_change_requests(&after_comment).expect("prior opinion remains"),
        1
    );
    let after_approval = [opinion(2, 10, "2026-10-02T00:00:00Z", "APPROVED")];
    assert_eq!(
        outstanding_change_requests(&after_approval).expect("approval supersedes request"),
        0
    );
}

#[test]
fn dismissed_rest_review_is_pruned_from_authoritative_opinion_set() {
    let all_rest_reviews = [opinion(1, 10, "2026-10-01T00:00:00Z", "DISMISSED")];
    let latest_opinionated_reviews = [];
    assert_eq!(
        outstanding_change_requests(&latest_opinionated_reviews)
            .expect("dismissed opinion removed by GitHub"),
        0
    );
    assert_eq!(all_rest_reviews[0]["state"], "DISMISSED");
}

#[test]
fn review_state_order_does_not_depend_on_page_or_response_order() {
    let approved = opinion(2, 10, "2026-10-02T00:00:00Z", "APPROVED");
    let requested = opinion(1, 11, "2026-10-01T00:00:00Z", "CHANGES_REQUESTED");
    assert_eq!(
        outstanding_change_requests(&[approved.clone(), requested.clone()])
            .expect("unordered opinions"),
        outstanding_change_requests(&[requested, approved]).expect("ordered opinions")
    );
}

#[test]
fn unknown_review_states_fail_closed() {
    for state in ["REOPENED", "COMMENTED", "DISMISSED"] {
        assert!(
            outstanding_change_requests(&[opinion(1, 10, "2026-10-01T00:00:00Z", state)]).is_err()
        );
    }
}

#[test]
fn opinion_query_fetches_numeric_identity_for_bot_reviewers() {
    let api = TapMergeGitHub::with_test_transport(|args| {
        assert!(args.iter().any(|arg| {
            arg.to_str()
                .is_some_and(|text| text.contains("... on Bot{databaseId}"))
        }));
        let response = serde_json::json!({"data": {"repository": {"pullRequest": {
            "latestOpinionatedReviews": {
                "nodes": [{"id": "review-node", "state": "CHANGES_REQUESTED",
                    "author": {"__typename": "Bot", "databaseId": 199_175_422}}],
                "pageInfo": {"hasNextPage": false, "endCursor": null}
            }
        }}}});
        Ok(super::super::github::ApiResponse {
            status: 200,
            body: serde_json::to_vec(&response).expect("response JSON"),
        })
    });
    let opinions = paged_opinionated_reviews(&api, "owner/tap", 21).expect("bot identity");
    assert_eq!(outstanding_change_requests(&opinions).expect("opinion"), 1);
}

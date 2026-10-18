use super::*;

#[path = "homebrew_merge_gate_history_tests.rs"]
mod history;

fn fixture() -> (TapMergePolicy, TapMergeObservation) {
    let policy = TapMergePolicy {
        repository: "example/tap".to_owned(),
        writer_app_id: 1,
        completion_app_id: 2,
        completion_check_name: "Review Complete".to_owned(),
        expected_reviewer_app_ids: BTreeSet::from([3, 4]),
    };
    let head = "a".repeat(40);
    let base = "b".repeat(40);
    let feedback = TapFeedbackSnapshot {
        repository: "example/tap".to_owned(),
        pull_request_number: 42,
        head_sha: head.clone(),
        records_digest: format!("b3-{}", "c".repeat(64)),
        unresolved_threads: 0,
        unaddressed_findings: 0,
        outstanding_change_requests: 0,
    };
    let completion = TapReviewCompletion {
        repository: "example/tap".to_owned(),
        pull_request_number: 42,
        check_name: policy.completion_check_name.clone(),
        app_id: 2,
        head_sha: head.clone(),
        completed_successfully: true,
        feedback_digest: feedback.canonical_digest().expect("digest"),
        completed_reviewer_app_ids: BTreeSet::from([3, 4]),
    };
    let checks = [("DCO", 5), ("Required", 5), ("Review Complete", 2)]
        .into_iter()
        .map(|(name, app_id)| TapRequiredCheck {
            name: name.to_owned(),
            app_id,
            required_app_id: Some(app_id),
            head_sha: head.clone(),
            completed_successfully: true,
        })
        .collect();
    (
        policy,
        TapMergeObservation {
            pull_request_number: 42,
            validated_base_sha: base.clone(),
            current_base_sha: base,
            validated_head_sha: head.clone(),
            current_head_sha: head,
            expected_open_pull_request: true,
            server_protection: TapServerMergeProtection::StrictWithoutWriterBypass,
            required_checks_complete: true,
            checks,
            feedback,
            completion: Some(completion),
            historical_feedback_complete: true,
            historical_feedback: Vec::new(),
        },
    )
}

#[test]
fn exact_head_success_requires_trusted_independent_completion() {
    let (policy, mut observed) = fixture();
    assert!(validate_tap_merge_gate(&policy, &observed).is_ok());
    observed.completion = None;
    assert!(
        validate_tap_merge_gate(&policy, &observed).is_err(),
        "silence is not completion"
    );
}

#[test]
fn exact_dco_and_app_bound_completion_must_be_server_protected() {
    for change in 0..5 {
        let (policy, mut observed) = fixture();
        match change {
            0 => observed.checks.retain(|check| check.name != "DCO"),
            1 => {
                observed
                    .checks
                    .iter_mut()
                    .find(|check| check.name == "DCO")
                    .expect("DCO")
                    .name = "DCO validation".to_owned();
            }
            2 => observed
                .checks
                .retain(|check| check.name != "Review Complete"),
            3 => {
                let check = observed
                    .checks
                    .iter_mut()
                    .find(|check| check.name == "Review Complete")
                    .expect("completion check");
                check.name = "Review Complete extra".to_owned();
            }
            _ => {
                let check = observed
                    .checks
                    .iter_mut()
                    .find(|check| check.name == "Review Complete")
                    .expect("completion check");
                check.app_id = 3;
                check.required_app_id = Some(3);
            }
        }
        assert!(
            validate_tap_merge_gate(&policy, &observed).is_err(),
            "change {change}"
        );
    }
}

#[test]
fn base_head_checks_and_credential_changes_block_merge() {
    for change in 0..9 {
        let (mut policy, mut observed) = fixture();
        match change {
            0 => observed.current_base_sha = "d".repeat(40),
            1 => observed.current_head_sha = "d".repeat(40),
            2 => observed.required_checks_complete = false,
            3 => observed.checks[0].completed_successfully = false,
            4 => observed.checks[0].app_id = 6,
            5 => policy.completion_app_id = policy.writer_app_id,
            6 => {
                policy
                    .expected_reviewer_app_ids
                    .insert(policy.writer_app_id);
            }
            7 => observed.checks[0].required_app_id = None,
            _ => observed.expected_open_pull_request = false,
        }
        assert!(
            validate_tap_merge_gate(&policy, &observed).is_err(),
            "change {change}"
        );
    }
}

#[test]
fn completion_app_cannot_also_serve_as_a_required_reviewer() {
    let (mut policy, mut observed) = fixture();
    policy
        .expected_reviewer_app_ids
        .insert(policy.completion_app_id);
    let completion = observed.completion.clone().expect("fixture completion");
    observed.completion = Some(TapReviewCompletion {
        completed_reviewer_app_ids: policy.expected_reviewer_app_ids.clone(),
        ..completion
    });
    assert!(validate_tap_merge_gate(&policy, &observed).is_err());
}

#[test]
fn feedback_arriving_after_completion_invalidates_the_gate() {
    let (policy, mut observed) = fixture();
    observed.feedback.records_digest = format!("b3-{}", "d".repeat(64));
    assert!(validate_tap_merge_gate(&policy, &observed).is_err());
}

#[test]
fn unfinished_feedback_cannot_be_hidden_by_a_successful_completion_check() {
    for change in 0..3 {
        let (policy, mut observed) = fixture();
        match change {
            0 => observed.feedback.unresolved_threads = 1,
            1 => observed.feedback.unaddressed_findings = 1,
            _ => observed.feedback.outstanding_change_requests = 1,
        }
        // Even an authenticated success over this same snapshot cannot waive feedback.
        observed
            .completion
            .as_mut()
            .expect("completion")
            .feedback_digest = observed.feedback.canonical_digest().expect("digest");
        assert!(validate_tap_merge_gate(&policy, &observed).is_err());
    }
}

#[test]
fn different_app_reviewer_set_or_stale_completion_is_rejected() {
    for change in 0..4 {
        let (policy, mut observed) = fixture();
        let completion = observed.completion.as_mut().expect("completion");
        match change {
            0 => completion.app_id = 7,
            1 => completion.head_sha = "d".repeat(40),
            2 => {
                completion.completed_reviewer_app_ids.remove(&4);
            }
            _ => completion.completed_successfully = false,
        }
        assert!(validate_tap_merge_gate(&policy, &observed).is_err());
    }
}

#[test]
fn server_must_close_the_base_advance_race_without_writer_bypass() {
    let (policy, mut observed) = fixture();
    observed.server_protection = TapServerMergeProtection::MissingOrBypassable;
    assert!(validate_tap_merge_gate(&policy, &observed).is_err());
}

#[test]
fn shared_actions_app_cannot_attest_independent_review_completion() {
    let (mut policy, mut observed) = fixture();
    policy.completion_app_id = crate::homebrew_review_completion::GITHUB_ACTIONS_APP_ID;
    observed.completion.as_mut().expect("completion").app_id = policy.completion_app_id;
    assert!(validate_tap_merge_gate(&policy, &observed).is_err());
}

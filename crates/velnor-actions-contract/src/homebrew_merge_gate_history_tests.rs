use super::*;

fn prior(observed: &TapMergeObservation, number: u64) -> TapHistoricalFeedback {
    let mut feedback = observed.feedback.clone();
    feedback.pull_request_number = number;
    let mut completion = observed.completion.clone().expect("completed fixture");
    completion.pull_request_number = number;
    completion.feedback_digest = feedback.canonical_digest().expect("feedback digest");
    TapHistoricalFeedback {
        pull_request_number: number,
        head_sha: observed.current_head_sha.clone(),
        feedback,
        completion: Some(completion),
    }
}

#[test]
fn replacement_pr_requires_exhaustive_disposed_historical_feedback() {
    let (policy, mut observed) = fixture();
    observed.historical_feedback = vec![prior(&observed, 40), prior(&observed, 41)];
    assert!(validate_tap_merge_gate(&policy, &observed).is_ok());
    for change in 0..11 {
        let mut candidate = observed.clone();
        match change {
            0 => candidate.historical_feedback_complete = false,
            1 => candidate.historical_feedback[0].feedback.unresolved_threads = 1,
            2 => {
                candidate.historical_feedback[0]
                    .feedback
                    .unaddressed_findings = 1;
            }
            3 => {
                candidate.historical_feedback[0]
                    .feedback
                    .outstanding_change_requests = 1;
            }
            4 => candidate.historical_feedback[0].completion = None,
            5 => candidate.historical_feedback[0].head_sha = "d".repeat(40),
            6 => {
                candidate.historical_feedback[0].feedback.records_digest =
                    format!("b3-{}", "d".repeat(64));
            }
            7 => candidate.historical_feedback.swap(0, 1),
            8 => candidate.historical_feedback[1] = candidate.historical_feedback[0].clone(),
            9 => {
                candidate.historical_feedback[0] = prior(&candidate, candidate.pull_request_number);
            }
            10 => candidate.historical_feedback[0].feedback.repository = "attacker/tap".to_owned(),
            _ => unreachable!(),
        }
        assert!(
            validate_tap_merge_gate(&policy, &candidate).is_err(),
            "case {change}"
        );
    }
}

fn completed_fixture() -> (TapMergePolicy, TapCompletedPromotionObservation) {
    let (policy, observed) = fixture();
    let selected = prior(&observed, observed.pull_request_number);
    let history = vec![prior(&observed, 41)];
    (
        policy,
        TapCompletedPromotionObservation {
            pull_request: selected,
            validated_base_sha: observed.validated_base_sha.clone(),
            observed_pr_base_sha: observed.validated_base_sha,
            validated_head_sha: observed.validated_head_sha,
            merge_sha: "e".repeat(40),
            main: TapCompletedMainObservation {
                current_sha: "f".repeat(40),
                merge_is_ancestor: true,
                formula_matches_receipt: true,
            },
            expected_merged_pull_request: true,
            server_protection: observed.server_protection,
            required_checks_complete: true,
            checks: observed.checks,
            historical_feedback_complete: true,
            historical_feedback: history,
        },
    )
}

#[test]
fn completed_promotion_requires_actual_merge_ancestry_formula_and_exact_head_gates() {
    let (policy, observed) = completed_fixture();
    assert!(validate_tap_completed_promotion_gate(&policy, &observed).is_ok());
    for change in 0..14 {
        let mut candidate = observed.clone();
        match change {
            0 => candidate.expected_merged_pull_request = false,
            1 => candidate.main.merge_is_ancestor = false,
            2 => candidate.main.formula_matches_receipt = false,
            3 => candidate.observed_pr_base_sha = "c".repeat(40),
            4 => candidate.validated_head_sha = "c".repeat(40),
            5 => candidate.merge_sha.clear(),
            6 => candidate.main.current_sha.clear(),
            7 => candidate.server_protection = TapServerMergeProtection::MissingOrBypassable,
            8 => candidate.required_checks_complete = false,
            9 => candidate.checks.retain(|check| check.name != "DCO"),
            10 => candidate.pull_request.completion = None,
            11 => candidate.pull_request.feedback.unresolved_threads = 1,
            12 => candidate.historical_feedback_complete = false,
            13 => {
                candidate.historical_feedback[0]
                    .feedback
                    .outstanding_change_requests = 1;
            }
            _ => unreachable!(),
        }
        assert!(
            validate_tap_completed_promotion_gate(&policy, &candidate).is_err(),
            "case {change}"
        );
    }
}

#[test]
fn historical_feedback_cannot_replay_completion_from_another_pr() {
    let (policy, observed) = fixture();
    let mut historical = prior(&observed, 41);
    historical.completion = observed.completion;
    assert!(validate_tap_historical_feedback(&policy, &historical).is_err());
}

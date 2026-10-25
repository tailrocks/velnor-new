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


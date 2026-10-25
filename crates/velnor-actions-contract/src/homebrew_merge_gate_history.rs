use super::{
    ContractError, TapFeedbackSnapshot, TapMergePolicy, TapRequiredCheck, TapReviewCompletion,
    TapServerMergeProtection, invalid, is_lower_hex_len, validate_checks, validate_feedback,
    validate_policy,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapHistoricalFeedback {
    pub pull_request_number: u64,

    pub head_sha: String,

    pub feedback: TapFeedbackSnapshot,

    pub completion: Option<TapReviewCompletion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapCompletedMainObservation {
    pub current_sha: String,

    pub merge_is_ancestor: bool,

    pub formula_matches_receipt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapCompletedPromotionObservation {
    pub pull_request: TapHistoricalFeedback,

    pub validated_base_sha: String,

    pub observed_pr_base_sha: String,

    pub validated_head_sha: String,

    pub merge_sha: String,

    pub main: TapCompletedMainObservation,

    pub expected_merged_pull_request: bool,

    pub server_protection: TapServerMergeProtection,

    pub required_checks_complete: bool,

    pub checks: Vec<TapRequiredCheck>,

    pub historical_feedback_complete: bool,

    pub historical_feedback: Vec<TapHistoricalFeedback>,
}

pub fn validate_tap_historical_feedback(
    policy: &TapMergePolicy,
    observed: &TapHistoricalFeedback,
) -> Result<(), ContractError> {
    validate_policy(policy)?;
    if !is_lower_hex_len(&observed.head_sha, 40) {
        return Err(invalid("bad_historical_commit_identity"));
    }
    validate_feedback(
        policy,
        observed.pull_request_number,
        &observed.head_sha,
        &observed.feedback,
        observed.completion.as_ref(),
    )
}

pub fn validate_tap_completed_promotion_gate(
    policy: &TapMergePolicy,
    observed: &TapCompletedPromotionObservation,
) -> Result<(), ContractError> {
    validate_policy(policy)?;
    for sha in [
        &observed.validated_base_sha,
        &observed.observed_pr_base_sha,
        &observed.validated_head_sha,
        &observed.merge_sha,
        &observed.main.current_sha,
    ] {
        if !is_lower_hex_len(sha, 40) {
            return Err(invalid("bad_completed_commit_identity"));
        }
    }
    if observed.validated_base_sha != observed.observed_pr_base_sha
        || observed.validated_head_sha != observed.pull_request.head_sha
        || !observed.expected_merged_pull_request
        || !observed.main.merge_is_ancestor
        || !observed.main.formula_matches_receipt
    {
        return Err(invalid("unproven_completed_promotion"));
    }
    if observed.server_protection != TapServerMergeProtection::PullRequestProtected {
        return Err(invalid("server_merge_protection_missing"));
    }
    validate_checks(
        policy,
        &observed.pull_request.head_sha,
        observed.required_checks_complete,
        &observed.checks,
    )?;
    validate_tap_historical_feedback(policy, &observed.pull_request)?;
    validate_history(
        policy,
        observed.pull_request.pull_request_number,
        observed.historical_feedback_complete,
        &observed.historical_feedback,
    )
}

pub(super) fn validate_history(
    policy: &TapMergePolicy,
    current_pull_request: u64,
    complete: bool,
    history: &[TapHistoricalFeedback],
) -> Result<(), ContractError> {
    if !complete {
        return Err(invalid("historical_feedback_incomplete"));
    }
    let mut previous = 0;
    for prior in history {
        if prior.pull_request_number <= previous
            || prior.pull_request_number == current_pull_request
        {
            return Err(invalid("historical_feedback_identity_conflict"));
        }
        validate_tap_historical_feedback(policy, prior)?;
        previous = prior.pull_request_number;
    }
    Ok(())
}

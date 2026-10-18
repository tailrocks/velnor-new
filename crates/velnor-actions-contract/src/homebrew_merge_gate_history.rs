//! Historical feedback and authenticated completed-promotion admission.

use super::{
    ContractError, TapFeedbackSnapshot, TapMergePolicy, TapRequiredCheck, TapReviewCompletion,
    TapServerMergeProtection, invalid, is_lower_hex_len, validate_checks, validate_feedback,
    validate_policy,
};

/// Independently captured feedback for one same-receipt pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapHistoricalFeedback {
    /// Actual PR number in the configured tap repository.
    pub pull_request_number: u64,
    /// Actual current PR head, even when the PR is closed or its branch deleted.
    pub head_sha: String,
    /// Complete feedback, including outdated threads and general comments.
    pub feedback: TapFeedbackSnapshot,
    /// Independent exact-head completion over that complete snapshot.
    pub completion: Option<TapReviewCompletion>,
}

/// Current tap-main evidence captured together for a completed promotion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapCompletedMainObservation {
    /// Captured current tap main commit.
    pub current_sha: String,
    /// Independently proven ancestry from the merge commit to this captured main.
    pub merge_is_ancestor: bool,
    /// Independently validated formula at this main exactly matches the signed receipt.
    pub formula_matches_receipt: bool,
}

/// Authenticated evidence that a previous invocation already promoted this receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapCompletedPromotionObservation {
    /// Selected authenticated merged PR and its current complete feedback.
    pub pull_request: TapHistoricalFeedback,
    /// Original base reconstructed from the signed-receipt candidate proof.
    pub validated_base_sha: String,
    /// Actual selected PR base returned by GitHub.
    pub observed_pr_base_sha: String,
    /// Exact original formula commit reconstructed and independently validated.
    pub validated_head_sha: String,
    /// Actual accepted merge commit from the selected merged PR.
    pub merge_sha: String,
    /// Ancestry and exact canonical formula at the same captured main commit.
    pub main: TapCompletedMainObservation,
    /// Actual merged state and expected bot, repository, branch, and PR identity.
    pub expected_merged_pull_request: bool,
    /// Current server-enforced checks and absence of writer bypass.
    pub server_protection: TapServerMergeProtection,
    /// All active required checks were exhaustively resolved for the original head.
    pub required_checks_complete: bool,
    /// Exact-head results, including DCO and the protected completion App.
    pub checks: Vec<TapRequiredCheck>,
    /// Every same-receipt PR was enumerated, without truncation.
    pub historical_feedback_complete: bool,
    /// Sorted feedback for every other same-receipt PR.
    pub historical_feedback: Vec<TapHistoricalFeedback>,
}

/// Validate feedback disposition on an authenticated historical PR.
/// # Errors
/// Rejects malformed identity, unfinished reviews, and any unaddressed feedback.
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

/// Admit an already completed promotion without authorizing any mutation.
/// # Errors
/// Rejects fabricated merges, stale identity, downgrade, missing checks or history,
/// incomplete reviews, and a changed feedback snapshot after completion.
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

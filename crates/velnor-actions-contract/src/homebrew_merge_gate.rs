use crate::{ContractError, canonical_json_bytes, digest_b3, ids::is_lower_hex_len};
use std::collections::BTreeSet;

#[path = "homebrew_merge_gate_history.rs"]
mod history;
pub use history::{
    TapCompletedMainObservation, TapCompletedPromotionObservation, TapHistoricalFeedback,
    validate_tap_completed_promotion_gate, validate_tap_historical_feedback,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapRequiredCheck {
    pub name: String,

    pub app_id: u64,

    pub required_app_id: Option<u64>,

    pub head_sha: String,

    pub completed_successfully: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapReviewCompletion {
    pub repository: String,

    pub pull_request_number: u64,

    pub check_name: String,

    pub app_id: u64,

    pub head_sha: String,

    pub completed_successfully: bool,

    pub feedback_digest: String,

    pub completed_reviewer_app_ids: BTreeSet<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct TapFeedbackSnapshot {
    pub repository: String,

    pub pull_request_number: u64,

    pub head_sha: String,

    pub records_digest: String,

    pub unresolved_threads: u64,

    pub unaddressed_findings: u64,

    pub outstanding_change_requests: u64,
}

impl TapFeedbackSnapshot {
    pub fn canonical_digest(&self) -> Result<String, ContractError> {
        Ok(digest_b3(&canonical_json_bytes(self)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapMergePolicy {
    pub repository: String,

    pub writer_app_id: u64,

    pub completion_app_id: u64,

    pub completion_check_name: String,

    pub expected_reviewer_app_ids: BTreeSet<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapServerMergeProtection {
    PullRequestProtected,

    MissingOrBypassable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapMergeObservation {
    pub pull_request_number: u64,

    pub validated_base_sha: String,

    pub current_base_sha: String,

    pub validated_head_sha: String,

    pub current_head_sha: String,

    pub expected_open_pull_request: bool,

    pub server_protection: TapServerMergeProtection,

    pub required_checks_complete: bool,

    pub checks: Vec<TapRequiredCheck>,

    pub feedback: TapFeedbackSnapshot,

    pub completion: Option<TapReviewCompletion>,

    pub historical_feedback_complete: bool,

    pub historical_feedback: Vec<TapHistoricalFeedback>,
}

pub fn validate_tap_merge_gate(
    policy: &TapMergePolicy,
    observed: &TapMergeObservation,
) -> Result<(), ContractError> {
    validate_policy(policy)?;
    for sha in [
        &observed.validated_base_sha,
        &observed.current_base_sha,
        &observed.validated_head_sha,
        &observed.current_head_sha,
    ] {
        if !is_lower_hex_len(sha, 40) {
            return Err(invalid("bad_commit_identity"));
        }
    }
    if observed.validated_base_sha != observed.current_base_sha
        || observed.validated_head_sha != observed.current_head_sha
        || !observed.expected_open_pull_request
    {
        return Err(invalid("stale_or_unexpected_pull_request"));
    }
    if observed.server_protection != TapServerMergeProtection::PullRequestProtected {
        return Err(invalid("server_merge_protection_missing"));
    }
    validate_checks(
        policy,
        &observed.current_head_sha,
        observed.required_checks_complete,
        &observed.checks,
    )?;
    validate_feedback(
        policy,
        observed.pull_request_number,
        &observed.current_head_sha,
        &observed.feedback,
        observed.completion.as_ref(),
    )?;
    history::validate_history(
        policy,
        observed.pull_request_number,
        observed.historical_feedback_complete,
        &observed.historical_feedback,
    )
}

fn validate_feedback(
    policy: &TapMergePolicy,
    pull_request_number: u64,
    head_sha: &str,
    feedback: &TapFeedbackSnapshot,
    completion: Option<&TapReviewCompletion>,
) -> Result<(), ContractError> {
    if pull_request_number == 0
        || feedback.repository != policy.repository
        || feedback.pull_request_number != pull_request_number
        || feedback.head_sha != head_sha
        || feedback.unresolved_threads != 0
        || feedback.unaddressed_findings != 0
        || feedback.outstanding_change_requests != 0
        || !valid_digest(&feedback.records_digest)
    {
        return Err(invalid("unaddressed_feedback"));
    }
    let completion = completion.ok_or_else(|| invalid("review_completion_missing"))?;
    if completion.repository != policy.repository
        || completion.pull_request_number != pull_request_number
        || completion.app_id != policy.completion_app_id
        || completion.check_name != policy.completion_check_name
        || completion.head_sha != head_sha
        || !completion.completed_successfully
        || completion.completed_reviewer_app_ids != policy.expected_reviewer_app_ids
        || completion.feedback_digest != feedback.canonical_digest()?
    {
        return Err(invalid("review_completion_mismatch"));
    }
    Ok(())
}

fn validate_policy(policy: &TapMergePolicy) -> Result<(), ContractError> {
    if !crate::vcs::is_safe_github_repository(&policy.repository)
        || policy.writer_app_id == 0
        || policy.completion_app_id == 0
        || policy.completion_app_id == crate::homebrew_review_completion::GITHUB_ACTIONS_APP_ID
        || policy.writer_app_id == policy.completion_app_id
        || policy.completion_check_name.trim().is_empty()
        || policy.expected_reviewer_app_ids.is_empty()
        || policy.expected_reviewer_app_ids.contains(&0)
        || policy
            .expected_reviewer_app_ids
            .contains(&policy.writer_app_id)
        || policy
            .expected_reviewer_app_ids
            .contains(&policy.completion_app_id)
    {
        return Err(invalid("invalid_credential_boundary"));
    }
    Ok(())
}

fn validate_checks(
    policy: &TapMergePolicy,
    head_sha: &str,
    complete: bool,
    checks: &[TapRequiredCheck],
) -> Result<(), ContractError> {
    if !complete || checks.is_empty() {
        return Err(invalid("required_checks_missing"));
    }
    let mut names = BTreeSet::new();
    for check in checks {
        if check.name.is_empty()
            || !names.insert(check.name.as_str())
            || check.app_id == 0
            || check.required_app_id != Some(check.app_id)
            || check.head_sha != head_sha
            || !check.completed_successfully
        {
            return Err(invalid("required_check_failed_or_untrusted"));
        }
    }
    if !names.contains("DCO") {
        return Err(invalid("required_dco_check_missing"));
    }
    if !checks.iter().any(|check| {
        check.name == policy.completion_check_name
            && check.app_id == policy.completion_app_id
            && check.required_app_id == Some(policy.completion_app_id)
    }) {
        return Err(invalid("protected_completion_check_missing"));
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("b3-")
        .is_some_and(|digest| is_lower_hex_len(digest, 64))
}
fn invalid(reason: &'static str) -> ContractError {
    ContractError::identity("homebrew_merge_gate", reason)
}

//! Exact-head merge admission after independent feedback completion.
//!
//! This pure boundary consumes authenticated GitHub observations. Transport must
//! obtain check App IDs from GitHub, never from check output or PR-controlled data.
//! The trusted completion App binds its independent review to the complete current
//! feedback snapshot; a quiet period or absent comments is never completion.

use crate::{ContractError, canonical_json_bytes, digest_b3, ids::is_lower_hex_len};
use std::collections::BTreeSet;

/// Authenticated required check result on the proposed formula commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapRequiredCheck {
    /// Required check context read from the active repository rules.
    pub name: String,
    /// Actual check application ID reported by GitHub.
    pub app_id: u64,
    /// App binding imposed by repository rules; missing binding fails closed.
    pub required_app_id: Option<u64>,
    /// Exact commit checked by this result.
    pub head_sha: String,
    /// Only a terminal success satisfies this native promotion policy.
    pub completed_successfully: bool,
}

/// A check produced by the separately credentialed trusted completion App.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapReviewCompletion {
    /// Repository bound by the authenticated completion payload.
    pub repository: String,
    /// Pull request bound by the authenticated completion payload.
    pub pull_request_number: u64,
    /// Actual check name, not a name supplied in its output text.
    pub check_name: String,
    /// Actual check App ID reported by GitHub.
    pub app_id: u64,
    /// Actual check head SHA reported by GitHub.
    pub head_sha: String,
    /// Actual terminal success reported by GitHub.
    pub completed_successfully: bool,
    /// Digest attested by this trusted check over the complete feedback snapshot.
    pub feedback_digest: String,
    /// Reviewer App IDs whose terminal exact-head completion the check verified.
    pub completed_reviewer_app_ids: BTreeSet<u64>,
}

/// Complete normalized feedback captured immediately before merge admission.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct TapFeedbackSnapshot {
    /// Repository whose complete feedback was captured.
    pub repository: String,
    /// Pull request whose complete feedback was captured.
    pub pull_request_number: u64,
    /// Exact proposed commit to which review completion must apply.
    pub head_sha: String,
    /// Digest of all paginated review, comment, reply, thread, and disposition
    /// records, including outdated threads and general PR comments.
    pub records_digest: String,
    /// Number of unresolved threads, whether current or outdated.
    pub unresolved_threads: u64,
    /// Number of findings lacking an independently verified justified disposition.
    pub unaddressed_findings: u64,
    /// Number of reviews requesting changes without a verified disposition.
    pub outstanding_change_requests: u64,
}

impl TapFeedbackSnapshot {
    /// Canonical binding consumed by the trusted review-completion check.
    /// # Errors
    /// Returns an encoding error if canonical serialization fails.
    pub fn canonical_digest(&self) -> Result<String, ContractError> {
        Ok(digest_b3(&canonical_json_bytes(self)?))
    }
}

/// Runtime-resolved numeric credential and reviewer boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapMergePolicy {
    /// Exact tap repository from static workflow authority.
    pub repository: String,
    /// Separate writer App; it cannot attest its own review completion.
    pub writer_app_id: u64,
    /// Separate trusted review-completion App.
    pub completion_app_id: u64,
    /// Exact trusted check name from configuration.
    pub completion_check_name: String,
    /// Nonempty independently configured reviewer App set.
    pub expected_reviewer_app_ids: BTreeSet<u64>,
}

/// GitHub protection required to close the base-advance merge race.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapServerMergeProtection {
    /// Required checks enforce an up-to-date branch; the writer cannot bypass.
    StrictWithoutWriterBypass,
    /// Protection is absent, non-strict, bypassable, or unproven.
    MissingOrBypassable,
}

/// Authenticated state captured at the final merge boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapMergeObservation {
    /// Exact writer-owned pull request being considered for merge.
    pub pull_request_number: u64,
    /// Captured tap main SHA from the independently validated candidate.
    pub validated_base_sha: String,
    /// Current tap main SHA, re-fetched immediately before merge.
    pub current_base_sha: String,
    /// Formula commit SHA independently validated by the writer.
    pub validated_head_sha: String,
    /// Current PR head SHA, re-fetched immediately before merge.
    pub current_head_sha: String,
    /// A non-draft open PR owned by the configured writer installation.
    pub expected_open_pull_request: bool,
    /// Server-enforced up-to-date protection with no writer bypass.
    pub server_protection: TapServerMergeProtection,
    /// All required checks were exhaustively resolved from active branch rules.
    pub required_checks_complete: bool,
    /// Required checks, including DCO, from the current exact head.
    pub checks: Vec<TapRequiredCheck>,
    /// Full feedback snapshot, independently re-fetched at the current head.
    pub feedback: TapFeedbackSnapshot,
    /// Trusted exact-head completion check, if present.
    pub completion: Option<TapReviewCompletion>,
}

/// Validate the sole native squash-merge admission condition.
///
/// A successful result is valid only for a merge request carrying the exact
/// `validated_head_sha` precondition. A changed base requires preparation again;
/// it must never be silently rebased or merged using previously validated bytes.
/// # Errors
/// Rejects credential aliasing, stale base/head, failed/missing checks, unfinished
/// review, outstanding feedback, and a changed feedback snapshot after completion.
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
    if observed.server_protection != TapServerMergeProtection::StrictWithoutWriterBypass {
        return Err(invalid("server_merge_protection_missing"));
    }
    validate_checks(policy, observed)?;
    let feedback = &observed.feedback;
    if observed.pull_request_number == 0
        || feedback.repository != policy.repository
        || feedback.pull_request_number != observed.pull_request_number
        || feedback.head_sha != observed.current_head_sha
        || feedback.unresolved_threads != 0
        || feedback.unaddressed_findings != 0
        || feedback.outstanding_change_requests != 0
        || !valid_digest(&feedback.records_digest)
    {
        return Err(invalid("unaddressed_feedback"));
    }
    let completion = observed
        .completion
        .as_ref()
        .ok_or_else(|| invalid("review_completion_missing"))?;
    if completion.repository != policy.repository
        || completion.pull_request_number != observed.pull_request_number
        || completion.app_id != policy.completion_app_id
        || completion.check_name != policy.completion_check_name
        || completion.head_sha != observed.current_head_sha
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
    observed: &TapMergeObservation,
) -> Result<(), ContractError> {
    if !observed.required_checks_complete || observed.checks.is_empty() {
        return Err(invalid("required_checks_missing"));
    }
    let mut names = BTreeSet::new();
    for check in &observed.checks {
        if check.name.is_empty()
            || !names.insert(check.name.as_str())
            || check.app_id == 0
            || check.required_app_id != Some(check.app_id)
            || check.head_sha != observed.current_head_sha
            || !check.completed_successfully
        {
            return Err(invalid("required_check_failed_or_untrusted"));
        }
    }
    if !names.contains("DCO") {
        return Err(invalid("required_dco_check_missing"));
    }
    if !observed.checks.iter().any(|check| {
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

#[cfg(test)]
#[path = "homebrew_merge_gate_tests.rs"]
mod tests;

//! Closed check-output protocol for an independent exact-head completion App.

/// GitHub's shared Actions App cannot independently authenticate review completion.
/// Identity: `GET https://api.github.com/apps/github-actions`.
pub const GITHUB_ACTIONS_APP_ID: u64 = 15_368;

use crate::{
    ContractError, canonical_json_bytes, homebrew_merge_gate::TapReviewCompletion,
    ids::is_lower_hex_len,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Sole supported independent review-completion payload schema.
pub const TAP_REVIEW_COMPLETION_SCHEMA: &str = "velnor.review-completion.v1";

/// GitHub-authenticated check metadata, obtained independently of output text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapCompletionCheckMetadata {
    /// Actual check name from GitHub's check-run API.
    pub name: String,
    /// Actual numeric App identity from GitHub's check-run API.
    pub app_id: u64,
    /// Actual check head from GitHub's check-run API.
    pub head_sha: String,
    /// Actual completed/success result, never inferred from payload claims.
    pub completed_successfully: bool,
}

/// Canonical complete verdict emitted only by the independent trusted App.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapReviewCompletionPayload {
    /// Exact versioned discriminator.
    pub schema: String,
    /// Exact tap repository.
    pub repository: String,
    /// Exact PR database number within the repository.
    pub pull_request_number: u64,
    /// Exact reviewed formula commit.
    pub head_sha: String,
    /// Sole terminal successful verdict; unknown or incomplete states reject.
    pub verdict: TapReviewVerdict,
    /// Canonical digest of the complete current feedback snapshot.
    pub feedback_digest: String,
    /// Explicit finding disposition summary; it must be false.
    pub has_unaddressed_findings: bool,
    /// Sorted, unique, complete expected reviewer identities with evidence bindings.
    pub reviewers: Vec<TapCompletedReviewer>,
}

/// The only verdict which can be used for merge admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TapReviewVerdict {
    /// Every configured review and finding disposition has completed.
    Complete,
}

/// A trusted completion App's authenticated exact-head reviewer evidence binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapCompletedReviewer {
    /// Numeric reviewer App identity from configured policy and GitHub metadata.
    pub app_id: u64,
    /// Exact commit explicitly completed by that reviewer.
    pub head_sha: String,
    /// Digest of the authenticated terminal review and disposition records.
    pub evidence_digest: String,
    /// Explicit terminal completion; missing or false fails closed.
    pub complete: bool,
    /// Explicit finding disposition summary; it must be false.
    pub has_unaddressed_findings: bool,
}

/// Parse a trusted check's exact canonical output text into merge evidence.
///
/// This function does not authenticate GitHub transport or App identity. The
/// caller must fetch the unique check run from the expected repository and pass
/// its actual metadata. The merge gate separately compares configured App IDs,
/// expected reviewers, PR/head identity, and a freshly captured feedback digest.
/// # Errors
/// Rejects unknown/duplicate fields, noncanonical bytes, missing or incomplete
/// review identities, duplicate/unsorted reviewers, and any unresolved finding.
pub fn parse_tap_review_completion(
    metadata: &TapCompletionCheckMetadata,
    bytes: &[u8],
) -> Result<TapReviewCompletion, ContractError> {
    let value = crate::strict_json::parse_strict_json_bytes(bytes, 64 * 1024)?;
    let payload: TapReviewCompletionPayload =
        serde_json::from_value(value).map_err(|_| invalid("invalid_payload"))?;
    if canonical_json_bytes(&payload)? != bytes {
        return Err(invalid("noncanonical_payload"));
    }
    if payload.schema != TAP_REVIEW_COMPLETION_SCHEMA
        || !crate::vcs::is_safe_github_repository(&payload.repository)
        || payload.pull_request_number == 0
        || !is_lower_hex_len(&payload.head_sha, 40)
        || payload.head_sha != metadata.head_sha
        || metadata.app_id == 0
        || metadata.name.is_empty()
        || !metadata.completed_successfully
        || payload.has_unaddressed_findings
        || !valid_digest(&payload.feedback_digest)
        || payload.reviewers.is_empty()
    {
        return Err(invalid("incomplete_or_mismatched_verdict"));
    }
    let mut reviewers = BTreeSet::new();
    let mut previous = 0;
    for reviewer in &payload.reviewers {
        if reviewer.app_id <= previous
            || !reviewer.complete
            || reviewer.has_unaddressed_findings
            || reviewer.head_sha != payload.head_sha
            || !valid_digest(&reviewer.evidence_digest)
        {
            return Err(invalid("invalid_reviewer_completion"));
        }
        previous = reviewer.app_id;
        reviewers.insert(reviewer.app_id);
    }
    Ok(TapReviewCompletion {
        repository: payload.repository,
        pull_request_number: payload.pull_request_number,
        check_name: metadata.name.clone(),
        app_id: metadata.app_id,
        head_sha: metadata.head_sha.clone(),
        completed_successfully: metadata.completed_successfully,
        feedback_digest: payload.feedback_digest,
        completed_reviewer_app_ids: reviewers,
    })
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("b3-")
        .is_some_and(|digest| is_lower_hex_len(digest, 64))
}
fn invalid(reason: &'static str) -> ContractError {
    ContractError::identity("homebrew_review_completion", reason)
}

#[cfg(test)]
#[path = "homebrew_review_completion_tests.rs"]
mod tests;

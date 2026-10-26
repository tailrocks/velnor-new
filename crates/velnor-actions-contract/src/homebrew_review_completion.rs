// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const GITHUB_ACTIONS_APP_ID: u64 = 15_368;

use crate::{
    ContractError, canonical_json_bytes, homebrew_merge_gate::TapReviewCompletion,
    ids::is_lower_hex_len,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const TAP_REVIEW_COMPLETION_SCHEMA: &str = "velnor.review-completion.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapCompletionCheckMetadata {
    pub name: String,

    pub app_id: u64,

    pub head_sha: String,

    pub completed_successfully: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapReviewCompletionPayload {
    pub schema: String,

    pub repository: String,

    pub pull_request_number: u64,

    pub head_sha: String,

    pub verdict: TapReviewVerdict,

    pub feedback_digest: String,

    pub has_unaddressed_findings: bool,

    pub reviewers: Vec<TapCompletedReviewer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TapReviewVerdict {
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapCompletedReviewer {
    pub app_id: u64,

    pub head_sha: String,

    pub evidence_digest: String,

    pub complete: bool,

    pub has_unaddressed_findings: bool,
}

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

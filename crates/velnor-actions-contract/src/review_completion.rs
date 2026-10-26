// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{ContractError, canonical_json_bytes, config::ReviewCompletionProvider};
use serde::{Deserialize, Serialize};

pub const REVIEW_COMPLETION_POLICY_SCHEMA: &str = "velnor.review-completion-policy.v1";

pub const REVIEW_COMPLETION_WORKFLOW_PATH: &str = ".github/workflows/review-completion.yml";

pub const REVIEW_COMPLETION_ENVIRONMENT: &str = "review-completion";

pub const MAX_REVIEW_COMPLETION_POLICY_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCompletionPolicy {
    pub schema: String,

    pub repository: String,

    pub default_branch: String,

    pub provider: ReviewCompletionProvider,

    pub check_name: String,
}

impl ReviewCompletionPolicy {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != REVIEW_COMPLETION_POLICY_SCHEMA
            || !crate::vcs::is_safe_github_repository(&self.repository)
            || !crate::is_valid_branch_name(&self.default_branch)
            || !crate::config::valid_review_check_name(&self.check_name)
        {
            return Err(ContractError::identity(
                "review_completion",
                "invalid_policy",
            ));
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let bytes = canonical_json_bytes(self)?;
        crate::strict_json::check_document_size(bytes.len(), MAX_REVIEW_COMPLETION_POLICY_BYTES)?;
        Ok(bytes)
    }
}

pub fn parse_review_completion_policy(
    bytes: &[u8],
) -> Result<ReviewCompletionPolicy, ContractError> {
    let value =
        crate::strict_json::parse_strict_json_bytes(bytes, MAX_REVIEW_COMPLETION_POLICY_BYTES)?;
    let policy: ReviewCompletionPolicy = serde_json::from_value(value)
        .map_err(|_| ContractError::identity("review_completion", "invalid_policy_shape"))?;
    if policy.canonical_bytes()? != bytes {
        return Err(ContractError::identity(
            "review_completion",
            "noncanonical_policy",
        ));
    }
    Ok(policy)
}

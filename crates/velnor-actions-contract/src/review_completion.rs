//! Literal trusted-workflow policy for the source-free completion adapter.
use crate::{ContractError, canonical_json_bytes, config::ReviewCompletionProvider};
use serde::{Deserialize, Serialize};

/// Closed emitter policy schema.
pub const REVIEW_COMPLETION_POLICY_SCHEMA: &str = "velnor.review-completion-policy.v1";
/// Sole companion workflow path.
pub const REVIEW_COMPLETION_WORKFLOW_PATH: &str = ".github/workflows/review-completion.yml";
/// Fixed protected environment for emitter credentials.
pub const REVIEW_COMPLETION_ENVIRONMENT: &str = "review-completion";
/// Maximum literal policy bytes, independent of mutable feedback size.
pub const MAX_REVIEW_COMPLETION_POLICY_BYTES: usize = 16 * 1024;

/// Inventory-only trusted policy; never executable source or an asserted verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCompletionPolicy {
    /// Exact versioned policy discriminator.
    pub schema: String,
    /// Exact target repository derived from generation identity.
    pub repository: String,
    /// Default branch derived from generation identity.
    pub default_branch: String,
    /// Closed authenticated provider protocol.
    pub provider: ReviewCompletionProvider,
    /// Exact emitted check name.
    pub check_name: String,
}

impl ReviewCompletionPolicy {
    /// Validate the complete policy before API access or output.
    /// # Errors
    /// Rejects unknown schema and malformed identities.
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
    /// Encode bounded canonical literal bytes.
    /// # Errors
    /// Rejects malformed or oversized policy.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let bytes = canonical_json_bytes(self)?;
        crate::strict_json::check_document_size(bytes.len(), MAX_REVIEW_COMPLETION_POLICY_BYTES)?;
        Ok(bytes)
    }
}

/// Parse the exact bounded canonical native policy.
/// # Errors
/// Rejects duplicate/unknown fields, malformed policy and noncanonical bytes.
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

#[cfg(test)]
#[path = "review_completion_tests.rs"]
mod tests;

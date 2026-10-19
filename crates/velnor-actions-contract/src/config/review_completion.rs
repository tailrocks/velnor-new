//! Closed source-free adapter for the installed Codex review provider.
use super::distribution::{valid_app_id_variable, valid_review_check_name, valid_secret_name};
use crate::ContractError;
use serde::{Deserialize, Serialize};

/// Sole supported authenticated review provider protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewCompletionProvider {
    /// Installed Codex App's closed two-role version-one summaries.
    #[serde(rename = "codex-v1")]
    CodexV1,
}

/// Optional native completion emitter. Presence enables the fixed companion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCompletionConfig {
    /// Closed provider protocol; arbitrary providers are not supported.
    pub provider: ReviewCompletionProvider,
    /// Exact check name required by native downstream policy.
    pub check_name: String,
    /// Variable containing the independent emitter App ID.
    pub app_id_variable: String,
    /// Secret containing that App's private key, used only by the pinned mint.
    pub app_private_key_secret: String,
    /// Variable containing the actual installed Codex reviewer App ID.
    pub reviewer_app_id_variable: String,
}

impl ReviewCompletionConfig {
    /// Validate literal identities before any workflow is emitted.
    /// # Errors
    /// Rejects malformed check/variable/secret references or shared App bindings.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let bad = |field, problem| {
            ContractError::config(file, format!("workflow.review_completion.{field}"), problem)
        };
        if !valid_review_check_name(&self.check_name) {
            return Err(bad("check_name", "bad_review_check_name"));
        }
        if !valid_app_id_variable(&self.app_id_variable)
            || !valid_app_id_variable(&self.reviewer_app_id_variable)
        {
            return Err(bad("app_id_variable", "bad_app_id_variable"));
        }
        if !valid_secret_name(&self.app_private_key_secret) {
            return Err(bad("app_private_key_secret", "bad_app_private_key_secret"));
        }
        if self.app_id_variable == self.reviewer_app_id_variable {
            return Err(bad(
                "reviewer_app_id_variable",
                "emitter_must_be_independent",
            ));
        }
        Ok(())
    }
}

//! Native helper outcome evidence, separate from reusable task results.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    ContractError, HelperInvocation, validate_digest, validate_matrix_key, validate_run_key,
    validate_task_id,
};

/// Complete source and planned identity recorded before native execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperObligationBinding {
    /// Wire schema.
    pub schema: u32,
    /// Workflow run and attempt.
    pub run_key: String,
    /// Planned source revision.
    pub source_head: String,
    /// Planned matrix entry.
    pub matrix_key: String,
    /// Planned obligation.
    pub task_id: String,
    /// Planned execution identity.
    pub task_digest: String,
    /// Deterministic native step identity.
    pub helper_id: String,
    /// Exact source digest, arguments, and managed tool selectors.
    pub invocation: HelperInvocation,
    /// Exact environment authorized by the compiled source owner.
    pub environment: BTreeMap<String, String>,
}

impl HelperObligationBinding {
    /// Validate shape; execution authority requires compiled owner equality.
    /// # Errors
    /// Rejects malformed or unbound identities.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != 1 {
            return Err(ContractError::identity(
                "helper_obligation.schema",
                "unsupported_schema",
            ));
        }
        validate_run_key(&self.run_key)?;
        validate_matrix_key(&self.matrix_key)?;
        validate_task_id(&self.task_id)?;
        validate_digest(&self.task_digest)?;
        crate::cachekey::validate_semantic_text("source_head", &self.source_head)?;
        if self.helper_id != format!("velnor-helper-{}", self.matrix_key) {
            return Err(ContractError::identity("helper_id", "identity_mismatch"));
        }
        self.invocation.validate()?;
        if self.environment.len() > 2048
            || self.environment.iter().any(|(key, value)| {
                key.is_empty() || key.chars().any(char::is_control) || value.contains('\0')
            })
        {
            return Err(ContractError::identity(
                "helper_environment",
                "invalid_environment",
            ));
        }
        Ok(())
    }
}

/// Original GitHub native helper outcome, without continue-on-error rewriting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperObligationOutcome {
    /// Helper completed successfully.
    Success,
    /// Helper failed.
    Failure,
    /// Helper was cancelled.
    Cancelled,
    /// Helper was skipped by its execution gate.
    Skipped,
}

/// Terminal native helper evidence accompanying ordinary obligation coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperObligationReport {
    /// Exact binding recorded before execution.
    pub binding: HelperObligationBinding,
    /// Terminal upstream step outcome.
    pub outcome: HelperObligationOutcome,
}

impl HelperObligationReport {
    /// Validate the bound scope.
    /// # Errors
    /// Rejects malformed bindings.
    pub fn validate(&self) -> Result<(), ContractError> {
        self.binding.validate()
    }
}

//! Action API evidence, separate from compiler objects and task-result caches.

use serde::{Deserialize, Serialize};

use crate::{
    ContractError, validate_digest, validate_matrix_key, validate_run_key, validate_task_id,
};

/// Source and planned obligation bound before an action executes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionBinding {
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
    /// Logical validation action ID, separate from its later cache export.
    pub action_id: String,
    /// Exact upstream action commit.
    pub action_ref: String,
}

impl ActionBinding {
    /// Validate scope and deterministic action identity.
    /// # Errors
    /// Rejects malformed or unbound identities.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != 1 {
            return Err(ContractError::identity(
                "action.schema",
                "unsupported_schema",
            ));
        }
        validate_run_key(&self.run_key)?;
        validate_matrix_key(&self.matrix_key)?;
        validate_task_id(&self.task_id)?;
        validate_digest(&self.task_digest)?;
        if self.action_id != format!("velnor-action-{}", self.matrix_key) {
            return Err(ContractError::identity("action_id", "identity_mismatch"));
        }
        crate::cachekey::validate_semantic_text("source_head", &self.source_head)?;
        let Some((name, sha)) = self.action_ref.rsplit_once('@') else {
            return Err(ContractError::identity("action_ref", "unpinned"));
        };
        if name != "docker/build-push-action"
            || sha.len() != 40
            || !sha
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ContractError::identity("action_ref", "unsupported_action"));
        }
        Ok(())
    }
}

/// Terminal upstream action outcome; never a cache verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcome {
    /// Action completed successfully.
    Success,
    /// Action failed.
    Failure,
    /// Action was cancelled.
    Cancelled,
    /// Action was skipped by its execution gate.
    Skipped,
}

/// Terminal action evidence accompanying ordinary obligation coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionReport {
    /// Exact binding recorded before execution.
    pub binding: ActionBinding,
    /// Upstream outcome, captured without continue-on-error rewriting.
    pub outcome: ActionOutcome,
}

impl ActionReport {
    /// Validate the action's scope.
    /// # Errors
    /// Rejects malformed bindings.
    pub fn validate(&self) -> Result<(), ContractError> {
        self.binding.validate()
    }
}

//! Neutral planned identity for compiled helper execution.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{CompiledSourceHelper, ContractError, HelperInvocation, validate_matrix_key};

/// Exact compiled invocation and owned environment bound before task hashing.
///
/// Deserialization validates only shape. Execution authority always requires
/// equality with a record reconstructed by its closed compiled owner factory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperObligationDescriptor {
    /// Deterministic native step identity.
    pub id: String,
    /// Complete source digest, arguments, selectors and SDK launcher.
    pub invocation: HelperInvocation,
    /// Exact environment authorized by the compiled source owner.
    pub environment: BTreeMap<String, String>,
}

impl HelperObligationDescriptor {
    /// Bind a compiled owner's record to its deterministic matrix identity.
    /// # Errors
    /// Rejects malformed matrix identities, invocations or owned environments.
    pub fn from_compiled(
        record: &CompiledSourceHelper,
        matrix_key: &str,
    ) -> Result<Self, ContractError> {
        record.validate_binding()?;
        let value = Self {
            id: format!("velnor-helper-{matrix_key}"),
            invocation: record.invocation().clone(),
            environment: record.environment().clone(),
        };
        value.validate(matrix_key)?;
        Ok(value)
    }

    /// Validate shape without granting execution authority to wire data.
    /// # Errors
    /// Rejects mismatched helper identities and malformed owned environment.
    pub fn validate(&self, matrix_key: &str) -> Result<(), ContractError> {
        validate_matrix_key(matrix_key)?;
        if self.id != format!("velnor-helper-{matrix_key}") {
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

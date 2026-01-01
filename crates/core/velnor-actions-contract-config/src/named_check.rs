//! Typed identity for opaque repository-owned checks.
use velnor_actions_contract::extension_schemas::NAMED_CHECK_EXTENSION_SCHEMA;
use velnor_actions_contract::{ContractError, StackExtension, validate_digest};

use crate::config::MiseCheck;
use serde::{Deserialize, Serialize};

/// Complete named check definition and resolved task configuration identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedCheckIdentityExtension {
    /// Exact configured execution definition.
    pub check: MiseCheck,
    /// Digest of the complete resolved repository task configuration.
    pub task_config_digest: String,
    /// Full resolved qualified tool, dependency and installation-option fingerprint.
    pub qualification_digest: String,
}
impl NamedCheckIdentityExtension {
    /// Validate every semantic slot.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        self.check.validate("stack_extension", "check")?;
        validate_digest(&self.task_config_digest)?;
        validate_digest(&self.qualification_digest)
    }
    /// Build the standard extension envelope.
    /// # Errors
    pub fn to_stack_extension(&self) -> Result<StackExtension, ContractError> {
        self.validate()?;
        Ok(StackExtension {
            schema: NAMED_CHECK_EXTENSION_SCHEMA.to_owned(),
            data: serde_json::to_value(self)
                .map_err(|e| ContractError::CanonicalJson(e.to_string()))?,
        })
    }
}
/// Validate a typed envelope; unexpected data fields fail closed.
/// # Errors
pub fn validate_named_check_extension(extension: &StackExtension) -> Result<(), ContractError> {
    if extension.schema != NAMED_CHECK_EXTENSION_SCHEMA {
        return Err(ContractError::identity(
            "stack_extension.schema",
            "unknown_schema",
        ));
    }
    let typed: NamedCheckIdentityExtension = serde_json::from_value(extension.data.clone())
        .map_err(|e| ContractError::identity("stack_extension.data", e.to_string()))?;
    typed.validate()
}

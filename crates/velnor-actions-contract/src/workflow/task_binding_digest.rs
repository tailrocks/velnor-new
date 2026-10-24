use serde::{Deserialize, Serialize};

use crate::{errors::ContractError, validate_digest};

/// Digest for a task command with project-tool references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskBindingDigest(String);

impl TaskBindingDigest {
    /// Create a binding digest from a validated digest value.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not a digest.
    pub fn new(value: String) -> Result<Self, ContractError> {
        validate_digest(&value)?;
        Ok(Self(value))
    }

    /// Return the encoded digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

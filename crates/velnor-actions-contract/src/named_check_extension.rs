use super::NAMED_CHECK_EXTENSION_SCHEMA;
use crate::{ContractError, MiseCheck, StackExtension, validate_digest};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedCheckIdentityExtension {
    pub check: MiseCheck,

    pub task_config_digest: String,

    pub qualification_digest: String,
}
impl NamedCheckIdentityExtension {
    pub fn validate(&self) -> Result<(), ContractError> {
        self.check.validate("stack_extension", "check")?;
        validate_digest(&self.task_config_digest)?;
        validate_digest(&self.qualification_digest)
    }

    pub fn to_stack_extension(&self) -> Result<StackExtension, ContractError> {
        self.validate()?;
        Ok(StackExtension {
            schema: NAMED_CHECK_EXTENSION_SCHEMA.to_owned(),
            data: serde_json::to_value(self)
                .map_err(|e| ContractError::CanonicalJson(e.to_string()))?,
        })
    }
}

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

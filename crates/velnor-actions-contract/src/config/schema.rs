use super::VelnorConfig;
use crate::errors::ContractError;

impl VelnorConfig {
    /// Schema 1 rejects routing. Schema 2 requires `[execution]`.
    pub(super) fn check_schema(&self, file: &str) -> Result<(), ContractError> {
        match self.schema {
            1 => {
                if self.execution.is_some() {
                    return Err(ContractError::config(
                        file,
                        "execution",
                        "schema1_rejects_execution",
                    ));
                }
                Ok(())
            }
            2 => {
                let execution = self
                    .execution
                    .as_ref()
                    .ok_or_else(|| ContractError::config(file, "execution", "missing_execution"))?;
                execution.validate(file)
            }
            _ => Err(ContractError::UnsupportedSchema {
                field: "schema",
                found: self.schema.to_string(),
                expected: "1 or 2",
            }),
        }
    }
}

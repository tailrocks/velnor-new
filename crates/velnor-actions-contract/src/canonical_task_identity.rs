//! Validation of the neutral task identity and compiled helper descriptor.

use super::{ContractError, TaskIdentity, normalize_posix_path, validate_digest};

impl TaskIdentity {
    /// Parse identity JSON, rejecting duplicate keys (cache §1).
    /// # Errors
    pub fn parse_json(text: &str) -> Result<Self, ContractError> {
        let value = crate::strict_json::parse_strict_json(text)?;
        serde_json::from_value(value).map_err(|err| ContractError::CanonicalJson(err.to_string()))
    }

    /// Validate relative paths, task ID, and digest shapes.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != 1 {
            return Err(ContractError::UnsupportedSchema {
                field: "schema_version",
                found: self.schema_version.to_string(),
                expected: "1",
            });
        }
        crate::ids::validate_task_id(&self.task_id)
            .map_err(|_| ContractError::identity("task_id", "bad_task_id"))?;
        if let Some(helper) = &self.helper_obligation {
            let matrix_id = crate::matrix_id_for_task_group(&self.stack_id, &self.task_id)?;
            helper.validate(&crate::matrix_key_for_id(&matrix_id)?)?;
        }
        if let Some(recipe) = &self.native_recipe {
            recipe.validate()?;
        }
        if self
            .helper_obligation
            .as_ref()
            .and_then(|helper| helper.invocation.native_validation_descriptor())
            != self.native_recipe.as_ref()
        {
            return Err(ContractError::identity(
                "native_recipe",
                "helper_recipe_mismatch",
            ));
        }
        for field in [&self.project_root, &self.working_dir, &self.component_id] {
            normalize_posix_path(field)?;
        }
        for input in &self.inputs {
            normalize_posix_path(&input.path)?;
            validate_digest(&input.digest)?;
        }
        if self.argv.iter().any(|arg| arg.starts_with('/')) {
            return Err(ContractError::identity("argv", "absolute_path"));
        }
        self.validate_dependencies()?;
        self.vcs.validate()?;
        for name in self.environment.keys() {
            if crate::secrets::is_secret_env_name(name) {
                return Err(ContractError::identity(
                    "environment",
                    format!("secret_env:{name}"),
                ));
            }
        }
        Ok(())
    }

    /// Validate dependency task IDs are well-formed and sorted.
    fn validate_dependencies(&self) -> Result<(), ContractError> {
        for dep in &self.dependencies {
            crate::ids::validate_task_id(dep)?;
        }
        if self.dependencies.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(ContractError::identity("dependencies", "must_be_sorted"));
        }
        Ok(())
    }
}

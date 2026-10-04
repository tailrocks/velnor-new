//! Matrix entry construction and validation.

use super::{ContractError, ExecuteTaskIds, MatrixEntry, validate_matrix_run};
use crate::canonical::{normalize_posix_path, validate_digest};
use crate::config::VelnorConfig;
use crate::ids::{
    artifact_id_for_crate_job, matrix_id_for_task_group, matrix_key_for_id, report_id_for_matrix,
    validate_id, validate_run_key,
};

impl MatrixEntry {
    /// Build an entry, deriving `id`, `matrix_key`, `report_id`, `artifact_id`.
    /// # Errors
    #[expect(
        clippy::too_many_arguments,
        reason = "entry identity needs all nine inputs at once"
    )]
    pub fn derive(
        stack_id: &str,
        task_group_id: &str,
        run: &str,
        task_digest: &str,
        adapter_metadata: serde_json::Value,
        execute_task_ids: ExecuteTaskIds,
        input_digest: &str,
        run_key: &str,
        job_id: &str,
    ) -> Result<Self, ContractError> {
        let id = matrix_id_for_task_group(stack_id, task_group_id)?;
        let matrix_key = matrix_key_for_id(&id)?;
        validate_run_key(run_key)?;
        validate_digest(input_digest)?;
        validate_matrix_run(run)?;
        validate_digest(task_digest)?;
        execute_task_ids.validate()?;
        Ok(Self {
            report_id: report_id_for_matrix(run_key, &matrix_key)?,
            artifact_id: artifact_id_for_crate_job(run_key, job_id)?,
            id,
            matrix_key,
            job_id: job_id.to_owned(),
            stack_id: stack_id.to_owned(),
            task_id: task_group_id.to_owned(),
            run: run.to_owned(),
            task_digest: task_digest.to_owned(),
            adapter_metadata,
            execute_task_ids,
            input_digest: input_digest.to_owned(),
            native_recipe: None,
            cache_ids: None,
            declared_outputs: Vec::new(),
            test_run: Vec::new(),
        })
    }
    /// Validate derivations, digests, and task references for a run key.
    /// # Errors
    pub fn validate(&self, run_key: &str) -> Result<(), ContractError> {
        validate_id(&self.id)?;
        validate_run_key(run_key)?;
        let expect_key = matrix_key_for_id(&self.id)?;
        if expect_key != self.matrix_key {
            return Err(ContractError::identity("matrix_key", "key_mismatch"));
        }
        if report_id_for_matrix(run_key, &self.matrix_key)? != self.report_id {
            return Err(ContractError::identity("report_id", "report_mismatch"));
        }
        if artifact_id_for_crate_job(run_key, &self.job_id)? != self.artifact_id {
            return Err(ContractError::identity("artifact_id", "artifact_mismatch"));
        }
        validate_digest(&self.input_digest)?;
        validate_matrix_run(&self.run)?;
        validate_digest(&self.task_digest)?;
        self.execute_task_ids.validate()?;
        if let Some(recipe) = &self.native_recipe {
            recipe.validate()?;
        }
        for output in &self.declared_outputs {
            normalize_posix_path(output)?;
        }
        for task_ref in &self.test_run {
            task_ref.validate()?;
        }
        if !VelnorConfig::REGISTERED_STACKS.contains(&self.stack_id.as_str()) {
            return Err(ContractError::identity("stack_id", "unregistered_stack"));
        }
        if let Some(cache_ids) = &self.cache_ids {
            cache_ids.validate()?;
        }
        let expect_id = matrix_id_for_task_group(&self.stack_id, &self.task_id)?;
        if expect_id != self.id {
            return Err(ContractError::identity("id", "id_mismatch"));
        }
        Ok(())
    }
}

//! Matrix-entry identity, including typed variants for paired check lanes.

use super::cache_ids::EntryCacheIds;
use super::execute::{ExecuteTaskIds, ExecuteTaskRef};
use super::lanes::NamedCheckLaneVariant;
use serde::{Deserialize, Serialize};
use velnor_actions_contract::canonical::{normalize_posix_path, validate_digest};
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract::ids::{
    artifact_id_for_crate_job, matrix_id_for_task_group, matrix_key_for_id, report_id_for_matrix,
    validate_id, validate_run_key,
};
use velnor_actions_contract_config::config::VelnorConfig;

/// One `matrix.include` entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixEntry {
    /// Stable matrix ID (`stack:<sid>|task:<tgid>`).
    pub id: String,
    /// Derived matrix key.
    pub matrix_key: String,
    /// Registered detector ID.
    pub stack_id: String,
    /// Stable logical matrix task-group ID.
    pub task_id: String,
    /// Typed lane identity for a paired named check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane_variant: Option<NamedCheckLaneVariant>,
    /// Fixed single-line shell command the leg executes (`matrix.run`).
    pub run: String,
    /// Obligation task digest binding the leg's reports (`b3-` + 64 hex).
    pub task_digest: String,
    /// Opaque stack-adapter metadata.
    pub adapter_metadata: serde_json::Value,
    /// Executable obligations for this entry.
    pub execute_task_ids: ExecuteTaskIds,
    /// Entry input digest.
    pub input_digest: String,
    /// Derived report ID.
    pub report_id: String,
    /// Owning job ID (crate job or plan job); selects the artifact below.
    pub job_id: String,
    /// Derived job artifact name carrying this entry's reports.
    pub artifact_id: String,
    /// Cache identity digests recorded in the plan (cache §1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_ids: Option<EntryCacheIds>,
    /// Declared task outputs reuse verification covers (cache §3).
    #[serde(default)]
    pub declared_outputs: Vec<String>,
    /// Per-shard test-run refs; empty for non-test entries (par §8).
    #[serde(default)]
    pub test_run: Vec<ExecuteTaskRef>,
}

impl MatrixEntry {
    /// Build a single-lane entry, deriving all report and artifact identities.
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
        Self::derive_for_lane(
            stack_id,
            task_group_id,
            run,
            task_digest,
            adapter_metadata,
            execute_task_ids,
            input_digest,
            run_key,
            job_id,
            None,
        )
    }

    /// Build a named-check entry bound to one hosted or Scale Set lane.
    /// The logical `task_id` stays shared; matrix, report, and artifact IDs
    /// derive from the lane variant and exact emitted job ID.
    /// # Errors
    #[expect(
        clippy::too_many_arguments,
        reason = "entry identity needs all ten inputs at once"
    )]
    pub fn derive_for_lane(
        stack_id: &str,
        task_group_id: &str,
        run: &str,
        task_digest: &str,
        adapter_metadata: serde_json::Value,
        execute_task_ids: ExecuteTaskIds,
        input_digest: &str,
        run_key: &str,
        job_id: &str,
        lane_variant: Option<NamedCheckLaneVariant>,
    ) -> Result<Self, ContractError> {
        let identity_group = lane_task_group_id(task_group_id, lane_variant)?;
        let id = matrix_id_for_task_group(stack_id, &identity_group)?;
        let matrix_key = matrix_key_for_id(&id)?;
        validate_run_key(run_key)?;
        validate_digest(input_digest)?;
        super::plan::validate_matrix_run(run)?;
        validate_digest(task_digest)?;
        execute_task_ids.validate()?;
        validate_lane_binding(stack_id, job_id, lane_variant)?;
        Ok(Self {
            report_id: report_id_for_matrix(run_key, &matrix_key)?,
            artifact_id: artifact_id_for_crate_job(run_key, job_id)?,
            id,
            matrix_key,
            job_id: job_id.to_owned(),
            stack_id: stack_id.to_owned(),
            task_id: task_group_id.to_owned(),
            lane_variant,
            run: run.to_owned(),
            task_digest: task_digest.to_owned(),
            adapter_metadata,
            execute_task_ids,
            input_digest: input_digest.to_owned(),
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
        super::plan::validate_matrix_run(&self.run)?;
        validate_digest(&self.task_digest)?;
        self.execute_task_ids.validate()?;
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
        validate_lane_binding(&self.stack_id, &self.job_id, self.lane_variant)?;
        let identity_group = lane_task_group_id(&self.task_id, self.lane_variant)?;
        let expect_id = matrix_id_for_task_group(&self.stack_id, &identity_group)?;
        if expect_id != self.id {
            return Err(ContractError::identity("id", "id_mismatch"));
        }
        Ok(())
    }
}

fn lane_task_group_id(
    task_group_id: &str,
    variant: Option<NamedCheckLaneVariant>,
) -> Result<String, ContractError> {
    velnor_actions_contract::ids::validate_task_id(task_group_id)?;
    let Some(variant) = variant else {
        return Ok(task_group_id.to_owned());
    };
    let suffix = match variant {
        NamedCheckLaneVariant::Hosted => "hosted",
        NamedCheckLaneVariant::ScaleSet => "scale-set",
    };
    Ok(format!("{task_group_id}/lane-{suffix}"))
}

fn validate_lane_binding(
    stack_id: &str,
    job_id: &str,
    variant: Option<NamedCheckLaneVariant>,
) -> Result<(), ContractError> {
    velnor_actions_contract::ids::job_ids::validate_job_id(job_id)?;
    let Some(variant) = variant else {
        return Ok(());
    };
    if stack_id != "mise" {
        return Err(ContractError::identity(
            "matrix.lane_variant",
            "lane_variant_requires_named_check",
        ));
    }
    let suffix = match variant {
        NamedCheckLaneVariant::Hosted => super::lanes::HOSTED_SUFFIX,
        NamedCheckLaneVariant::ScaleSet => super::lanes::SCALE_SUFFIX,
    };
    if !job_id.starts_with("check-") || !job_id.ends_with(suffix) {
        return Err(ContractError::identity(
            "matrix.job_id",
            "lane_job_id_mismatch",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

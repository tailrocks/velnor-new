//! Additive, plan-bound runtime identity sidecar for one terminal task report.
//!
//! This is kept separate from schema-1 `TaskReport` bytes. The receipt records
//! observed GitHub job context and exact plan entry identity; it does not claim
//! numeric REST job or artifact IDs, which are not available at report time.
use serde::{Deserialize, Serialize};
use velnor_actions_contract::cachekey::validate_semantic_text;
use velnor_actions_contract::canonical::{canonical_json_bytes, validate_digest};
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract::ids::job_ids::validate_job_id;
use velnor_actions_contract::ids::{
    artifact_id_for_crate_job, matrix_key_for_id, task_report_id_for_task, validate_run_key,
    validate_task_id, validate_task_report_id,
};

use super::{MatrixEntry, Plan, canonical_plan_digest};

/// Directory name for task runtime receipt sidecars in a report artifact.
pub const TASK_RUNTIME_RECEIPTS_DIRECTORY: &str = "runtime-receipts";

/// Runtime identity observed in the executing GitHub Actions job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRuntimeIdentity {
    /// Exact `GITHUB_REPOSITORY` slug.
    pub repository: String,
    /// Canonical decimal `GITHUB_RUN_ID`.
    pub run_id: String,
    /// Positive `GITHUB_RUN_ATTEMPT`.
    pub run_attempt: u32,
    /// Full commit SHA from `GITHUB_SHA`.
    pub source_sha: String,
    /// Exact `GITHUB_WORKFLOW_REF`.
    pub workflow_ref: String,
    /// `GITHUB_JOB`, the workflow job key (not the REST job ID).
    pub workflow_job_key: String,
    /// Observed `RUNNER_NAME`; descriptive and not assumed unique.
    pub runner_name: String,
}

impl TaskRuntimeIdentity {
    /// Construct and validate one runtime identity from explicit environment values.
    ///
    /// # Errors
    /// Returns a contract error for incomplete or malformed values.
    pub fn new(
        repository: String,
        run_id: String,
        run_attempt: u32,
        source_sha: String,
        workflow_ref: String,
        workflow_job_key: String,
        runner_name: String,
    ) -> Result<Self, ContractError> {
        let value = Self {
            repository,
            run_id,
            run_attempt,
            source_sha,
            workflow_ref,
            workflow_job_key,
            runner_name,
        };
        value.validate()?;
        Ok(value)
    }

    /// Validate the observed values without assigning them to a plan.
    ///
    /// # Errors
    /// Returns a contract error for malformed values.
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_repository(&self.repository)?;
        validate_run_id(&self.run_id, self.run_attempt)?;
        validate_sha(&self.source_sha)?;
        validate_workflow_ref(&self.workflow_ref, &self.repository)?;
        validate_job_id(&self.workflow_job_key)?;
        validate_runner_name(&self.runner_name)
    }
}

/// Versioned sidecar binding observed job context to an exact plan entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRuntimeReceipt {
    /// Receipt schema version.
    pub schema: u32,
    /// Existing schema-1 task report identifier.
    pub task_report_id: String,
    /// GitHub run key (`r<run-id>-a<attempt>`).
    pub run_key: String,
    /// BLAKE3 digest of the complete canonical plan.
    pub plan_digest: String,
    /// Exact plan matrix identity.
    pub matrix_id: String,
    /// Exact plan matrix key.
    pub matrix_key: String,
    /// Exact logical task identity.
    pub task_id: String,
    /// Exact task digest from the plan obligation.
    pub task_digest: String,
    /// Plan's workflow job key for this entry.
    pub plan_job_id: String,
    /// Expected run-scoped upload artifact name, not a numeric artifact ID.
    pub report_artifact_name: String,
    /// Observed `GITHUB_REPOSITORY`.
    pub repository: String,
    /// Observed canonical `GITHUB_RUN_ID`.
    pub run_id: String,
    /// Observed positive `GITHUB_RUN_ATTEMPT`.
    pub run_attempt: u32,
    /// Observed `GITHUB_SHA`.
    pub source_sha: String,
    /// Observed `GITHUB_WORKFLOW_REF`.
    pub workflow_ref: String,
    /// Observed `GITHUB_JOB` workflow key, not the REST job ID.
    pub workflow_job_key: String,
    /// Observed `RUNNER_NAME`, which is not assumed unique.
    pub runner_name: String,
}

impl TaskRuntimeReceipt {
    /// Receipt schema version.
    pub const SCHEMA: u32 = 1;

    /// Derive a receipt only when observed runtime identity matches this plan entry.
    ///
    /// # Errors
    /// Returns a contract error for any plan, entry, task, or runtime mismatch.
    pub fn derive(
        plan: &Plan,
        entry: &MatrixEntry,
        task_report_id: &str,
        runtime: &TaskRuntimeIdentity,
    ) -> Result<Self, ContractError> {
        plan.validate()?;
        entry.validate(&plan.run_key)?;
        validate_plan_entry_membership(plan, entry)?;
        runtime.validate()?;
        validate_task_report_id(task_report_id)?;
        let value = Self {
            schema: Self::SCHEMA,
            task_report_id: task_report_id.to_owned(),
            run_key: plan.run_key.clone(),
            plan_digest: canonical_plan_digest(plan)?,
            matrix_id: entry.id.clone(),
            matrix_key: entry.matrix_key.clone(),
            task_id: entry.task_id.clone(),
            task_digest: entry.task_digest.clone(),
            plan_job_id: entry.job_id.clone(),
            report_artifact_name: entry.artifact_id.clone(),
            repository: runtime.repository.clone(),
            run_id: runtime.run_id.clone(),
            run_attempt: runtime.run_attempt,
            source_sha: runtime.source_sha.clone(),
            workflow_ref: runtime.workflow_ref.clone(),
            workflow_job_key: runtime.workflow_job_key.clone(),
            runner_name: runtime.runner_name.clone(),
        };
        value.validate_for_plan_entry(plan, entry)?;
        Ok(value)
    }

    /// Validate internal receipt identities and observed runtime values.
    ///
    /// # Errors
    /// Returns a contract error for inconsistent or malformed fields.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != Self::SCHEMA {
            return Err(ContractError::UnsupportedSchema {
                field: "task_runtime_receipt.schema",
                found: self.schema.to_string(),
                expected: "1",
            });
        }
        validate_run_key(&self.run_key)?;
        validate_digest(&self.plan_digest)?;
        validate_task_report_id(&self.task_report_id)?;
        validate_task_id(&self.task_id)?;
        validate_job_id(&self.plan_job_id)?;
        if matrix_key_for_id(&self.matrix_id)? != self.matrix_key {
            return Err(ContractError::identity(
                "task_runtime_receipt.matrix_key",
                "key_mismatch",
            ));
        }
        validate_digest(&self.task_digest)?;
        if task_report_id_for_task(&self.run_key, &self.matrix_key, &self.task_digest)?
            != self.task_report_id
        {
            return Err(ContractError::identity(
                "task_runtime_receipt.task_report_id",
                "report_mismatch",
            ));
        }
        if artifact_id_for_crate_job(&self.run_key, &self.plan_job_id)? != self.report_artifact_name
        {
            return Err(ContractError::identity(
                "task_runtime_receipt.report_artifact_name",
                "artifact_mismatch",
            ));
        }
        if self.workflow_job_key != self.plan_job_id {
            return Err(ContractError::identity(
                "task_runtime_receipt.workflow_job_key",
                "plan_job_mismatch",
            ));
        }
        let runtime = TaskRuntimeIdentity {
            repository: self.repository.clone(),
            run_id: self.run_id.clone(),
            run_attempt: self.run_attempt,
            source_sha: self.source_sha.clone(),
            workflow_ref: self.workflow_ref.clone(),
            workflow_job_key: self.workflow_job_key.clone(),
            runner_name: self.runner_name.clone(),
        };
        runtime.validate()?;
        if self.run_key != format!("r{}-a{}", self.run_id, self.run_attempt) {
            return Err(ContractError::identity(
                "task_runtime_receipt.run_key",
                "runtime_run_mismatch",
            ));
        }
        Ok(())
    }

    /// Validate this receipt against the authoritative plan entry.
    ///
    /// # Errors
    /// Returns a contract error for any mismatch.
    pub fn validate_for_plan_entry(
        &self,
        plan: &Plan,
        entry: &MatrixEntry,
    ) -> Result<(), ContractError> {
        self.validate()?;
        plan.validate()?;
        entry.validate(&plan.run_key)?;
        validate_plan_entry_membership(plan, entry)?;
        if self.run_key != plan.run_key
            || self.plan_digest != canonical_plan_digest(plan)?
            || self.matrix_id != entry.id
            || self.matrix_key != entry.matrix_key
            || self.task_id != entry.task_id
            || self.task_digest != entry.task_digest
            || self.plan_job_id != entry.job_id
            || self.report_artifact_name != entry.artifact_id
            || self.source_sha != plan.head
        {
            return Err(ContractError::identity(
                "task_runtime_receipt",
                "plan_entry_mismatch",
            ));
        }
        Ok(())
    }
}

fn validate_plan_entry_membership(plan: &Plan, entry: &MatrixEntry) -> Result<(), ContractError> {
    let mut matches = plan
        .matrix
        .include
        .iter()
        .filter(|planned| planned.id == entry.id);
    let planned = matches.next().ok_or_else(|| {
        ContractError::identity("task_runtime_receipt.plan_entry", "entry_not_in_plan")
    })?;
    if matches.next().is_some() || canonical_json_bytes(planned)? != canonical_json_bytes(entry)? {
        return Err(ContractError::identity(
            "task_runtime_receipt.plan_entry",
            "entry_mismatch",
        ));
    }

    let mut obligations = plan
        .obligations
        .iter()
        .filter(|obligation| obligation.task_id == entry.task_id);
    let obligation = obligations.next().ok_or_else(|| {
        ContractError::identity("task_runtime_receipt.plan_entry", "obligation_not_in_plan")
    })?;
    if obligations.next().is_some() || obligation.task_digest != entry.task_digest {
        return Err(ContractError::identity(
            "task_runtime_receipt.plan_entry",
            "obligation_digest_mismatch",
        ));
    }
    Ok(())
}

fn validate_run_id(run_id: &str, run_attempt: u32) -> Result<(), ContractError> {
    let parsed = run_id.parse::<u64>().ok();
    if parsed.is_none_or(|value| value == 0 || value.to_string() != run_id) || run_attempt == 0 {
        return Err(ContractError::identity(
            "task_runtime_receipt.run",
            "bad_run_identity",
        ));
    }
    Ok(())
}

fn validate_repository(value: &str) -> Result<(), ContractError> {
    let Some((owner, repository)) = value.split_once('/') else {
        return Err(ContractError::identity(
            "task_runtime_receipt.repository",
            "bad_repository_slug",
        ));
    };
    let valid_component = |component: &str| {
        !component.is_empty()
            && component.len() <= 100
            && component
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    };
    if value.len() > 201
        || value.matches('/').count() != 1
        || !valid_component(owner)
        || !valid_component(repository)
    {
        return Err(ContractError::identity(
            "task_runtime_receipt.repository",
            "bad_repository_slug",
        ));
    }
    Ok(())
}

fn validate_sha(value: &str) -> Result<(), ContractError> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ContractError::identity(
            "task_runtime_receipt.source_sha",
            "bad_commit_sha",
        ));
    }
    Ok(())
}

fn validate_workflow_ref(value: &str, repository: &str) -> Result<(), ContractError> {
    let prefix = format!("{repository}/.github/workflows/");
    let valid = value
        .strip_prefix(&prefix)
        .and_then(|path_ref| path_ref.split_once('@'))
        .is_some_and(|(path, reference)| {
            !path.is_empty()
                && !path.contains("..")
                && reference.starts_with("refs/")
                && reference.len() > "refs/".len()
                && !reference.chars().any(char::is_control)
        });
    if !valid {
        return Err(ContractError::identity(
            "task_runtime_receipt.workflow_ref",
            "bad_workflow_ref",
        ));
    }
    Ok(())
}

fn validate_runner_name(value: &str) -> Result<(), ContractError> {
    if value.trim().is_empty() || value.len() > 256 {
        return Err(ContractError::identity(
            "task_runtime_receipt.runner_name",
            "bad_runner_name",
        ));
    }
    validate_semantic_text("task_runtime_receipt.runner_name", value)
}

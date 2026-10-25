use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::cachekey::{ToolchainInputs, toolchain_id};
use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;
use crate::ids::{validate_matrix_key, validate_task_id};
use crate::validate_digest;
use crate::workflow::jobs::{
    TOFU_DISPLAY_PREFIX, TOFU_JOB_ID_PREFIX, is_safe_display_name, validate_job_id,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateObligation {
    pub task_id: String,

    pub kind: String,

    pub step_name: String,

    pub gated_by: Vec<String>,

    pub matrix_key: String,

    pub task_digest: String,

    pub toolchain_inputs: ToolchainInputs,

    pub run: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateJob {
    pub job_id: String,

    pub display_name: String,

    pub package_name: String,

    pub package_id: String,

    pub manifest: String,

    pub configuration: String,

    pub obligations: Vec<CrateObligation>,
}

impl CrateJob {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_job_id(&self.job_id)?;
        let prefix = if self.job_id.starts_with(TOFU_JOB_ID_PREFIX) {
            TOFU_DISPLAY_PREFIX
        } else {
            "Rust / "
        };
        if self.display_name.trim().is_empty()
            || !self.display_name.starts_with(prefix)
            || !is_safe_display_name(&self.display_name)
        {
            return Err(ContractError::identity(
                "crate_job.display_name",
                format!("bad_display:{}", self.job_id),
            ));
        }
        if self.obligations.is_empty() {
            return Err(ContractError::identity(
                "crate_job.obligations",
                format!("empty_obligations:{}", self.job_id),
            ));
        }
        let mut seen = BTreeSet::new();
        for obligation in &self.obligations {
            obligation.validate(&self.job_id)?;
            if seen.contains(&obligation.task_id) {
                return Err(ContractError::identity(
                    "crate_job.obligations",
                    format!("duplicate_task:{}", obligation.task_id),
                ));
            }
            for gate in &obligation.gated_by {
                if !seen.contains(gate) {
                    return Err(ContractError::identity(
                        "crate_job.gates",
                        format!("unordered_gate:{}:{}", obligation.task_id, gate),
                    ));
                }
            }
            seen.insert(obligation.task_id.clone());
        }
        Ok(())
    }
}

impl CrateObligation {
    fn validate(&self, job_id: &str) -> Result<(), ContractError> {
        validate_task_id(&self.task_id)?;
        validate_matrix_key(&self.matrix_key)?;
        validate_digest(&self.task_digest)?;
        let toolchain_id = toolchain_id(&self.toolchain_inputs)?;
        if task_digest_for_execution(&self.task_id, &self.run, &toolchain_id)? != self.task_digest {
            return Err(ContractError::identity(
                "crate_obligation.task_digest",
                format!("task_digest_mismatch:{job_id}"),
            ));
        }
        if self.kind.trim().is_empty() {
            return Err(ContractError::identity(
                "crate_obligation.kind",
                format!("empty_kind:{job_id}"),
            ));
        }
        if self.step_name.trim().is_empty()
            || self.step_name.contains('/')
            || self.step_name.contains("${{")
        {
            return Err(ContractError::identity(
                "crate_obligation.step_name",
                format!("bad_step_name:{job_id}"),
            ));
        }
        if self.step_name.contains(&self.task_id) {
            return Err(ContractError::identity(
                "crate_obligation.step_name",
                format!("task_id_in_label:{job_id}"),
            ));
        }
        if self.run.is_empty() || self.run.iter().any(|arg| arg.trim().is_empty()) {
            return Err(ContractError::identity(
                "crate_obligation.run",
                format!("bad_argv:{job_id}"),
            ));
        }
        Ok(())
    }
}

pub fn task_digest_for_execution(
    task_id: &str,
    argv: &[String],
    toolchain_id: &str,
) -> Result<String, ContractError> {
    validate_task_id(task_id)?;
    validate_digest(toolchain_id)?;
    if argv.is_empty()
        || argv
            .iter()
            .any(|arg| arg.is_empty() || arg.chars().any(|ch| matches!(ch, '\0' | '\n' | '\r')))
    {
        return Err(ContractError::identity(
            "crate_obligation.argv",
            "empty_argv",
        ));
    }
    Ok(digest_b3(&canonical_json_bytes(&TaskDigestInputs {
        task_id,
        argv,
        toolchain_id,
    })?))
}

#[derive(Serialize)]
struct TaskDigestInputs<'a> {
    task_id: &'a str,
    argv: &'a [String],
    toolchain_id: &'a str,
}

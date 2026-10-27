// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::cachekey::{ToolchainInputs, toolchain_id};
use crate::canonical::{canonical_json_bytes, digest_b3, normalize_posix_path};
use crate::errors::ContractError;
use crate::ids::{validate_matrix_key, validate_task_id};
use crate::validate_digest;
use crate::workflow::cache_identity::require_sorted_unique;
use crate::workflow::jobs::{
    TOFU_DISPLAY_PREFIX, TOFU_JOB_ID_PREFIX, is_safe_display_name, validate_job_id,
};
use crate::workflow::timeout::JobTimeout;

pub const GROUP_READER_SUFFIX: &str = "-read";

pub const GROUP_WRITER_SUFFIX: &str = "-write";

pub const SEED_JOB_ID_PREFIX: &str = "dependency-seed";

pub const MAX_GROUP_UNITS: usize = 64;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateGroupPlan {
    pub group_job_id: String,
    pub workspace_root: String,
    pub environment: String,
    pub unit_ids: Vec<String>,
    pub timeout_minutes: JobTimeout,
}

impl CrateGroupPlan {
    pub fn new(
        group_job_id: String,
        workspace_root: String,
        environment: String,
        unit_ids: Vec<String>,
        timeout_minutes: JobTimeout,
    ) -> Result<Self, ContractError> {
        let plan = Self {
            group_job_id,
            workspace_root,
            environment,
            unit_ids,
            timeout_minutes,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        validate_job_id(&self.group_job_id)?;
        if !self.workspace_root.is_empty() {
            normalize_posix_path(&self.workspace_root).map_err(|_| {
                ContractError::identity("crate_group.workspace_root", "bad_workspace_root")
            })?;
        }
        if self.environment.trim().is_empty() {
            return Err(ContractError::identity(
                "crate_group.environment",
                "missing_environment",
            ));
        }
        require_sorted_unique("crate_group.unit_ids", &self.unit_ids)?;
        if self.unit_ids.is_empty() {
            return Err(ContractError::identity(
                "crate_group.unit_ids",
                "empty_group_units",
            ));
        }
        if self.unit_ids.len() > MAX_GROUP_UNITS {
            return Err(ContractError::identity(
                "crate_group.unit_ids",
                "group_too_large",
            ));
        }
        for unit_id in &self.unit_ids {
            validate_task_id(unit_id)?;
        }
        self.timeout_minutes.validate()?;
        let reader = self.reader_job_id();
        let writer = self.writer_job_id();
        validate_job_id(&reader)?;
        validate_job_id(&writer)?;
        if reader == self.group_job_id || writer == self.group_job_id || reader == writer {
            return Err(ContractError::identity(
                "crate_group.branches",
                "branch_collision",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn reader_job_id(&self) -> String {
        reader_job_id_for(&self.group_job_id)
    }

    #[must_use]
    pub fn writer_job_id(&self) -> String {
        writer_job_id_for(&self.group_job_id)
    }
}

#[must_use]
pub fn reader_job_id_for(group_job_id: &str) -> String {
    format!("{group_job_id}{GROUP_READER_SUFFIX}")
}

#[must_use]
pub fn writer_job_id_for(group_job_id: &str) -> String {
    format!("{group_job_id}{GROUP_WRITER_SUFFIX}")
}

#[must_use]
pub fn is_seed_job_id(job_id: &str) -> bool {
    job_id == SEED_JOB_ID_PREFIX
        || job_id
            .strip_prefix(SEED_JOB_ID_PREFIX)
            .is_some_and(|rest| rest.starts_with('-') || rest.starts_with('_'))
}

pub fn validate_group_needs(needs: &[String], group_job_id: &str) -> Result<(), ContractError> {
    for need in needs {
        validate_job_id(need)?;
        if is_seed_job_id(need) {
            return Err(ContractError::identity(
                "crate_group.needs",
                format!("seed_dependency:{group_job_id}"),
            ));
        }
    }
    Ok(())
}

#[must_use]
pub fn group_timeout_for(measured_minutes: Option<u64>, bound: JobTimeout) -> JobTimeout {
    let ceiling = u64::from(bound.minutes().max(JobTimeout::MIN_MINUTES));
    let wanted = match measured_minutes {
        Some(minutes) => minutes.max(u64::from(JobTimeout::MIN_MINUTES)).min(ceiling),
        None => ceiling,
    };
    let clamped = wanted
        .min(u64::from(u16::MAX))
        .max(u64::from(JobTimeout::MIN_MINUTES));
    match u16::try_from(clamped) {
        Ok(minutes) => JobTimeout::new(minutes).unwrap_or(bound),
        Err(_) => bound,
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

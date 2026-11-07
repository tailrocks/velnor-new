// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_ids::EntryCacheIds;
use super::execute::{ExecuteTaskIds, ExecuteTaskRef};
use super::lanes::LaneVariant;
use super::platform::PlannedPlatform;
use crate::canonical::{normalize_posix_path, validate_digest};
use crate::config::VelnorConfig;
use crate::errors::ContractError;
use crate::ids::{
    artifact_id_for_crate_job, matrix_id_for_task_group, matrix_key_for_id, report_id_for_matrix,
    validate_id, validate_run_key,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixEntry {
    pub id: String,

    pub matrix_key: String,

    pub stack_id: String,

    pub task_id: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane_variant: Option<LaneVariant>,

    pub run: String,

    pub task_digest: String,

    pub adapter_metadata: serde_json::Value,

    pub execute_task_ids: ExecuteTaskIds,

    pub input_digest: String,

    pub report_id: String,

    pub job_id: String,

    pub artifact_id: String,

    pub planned_platform: PlannedPlatform,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_ids: Option<EntryCacheIds>,

    #[serde(default)]
    pub declared_outputs: Vec<String>,

    #[serde(default)]
    pub test_run: Vec<ExecuteTaskRef>,
}

impl MatrixEntry {
    #[expect(
        clippy::too_many_arguments,
        reason = "entry identity needs all ten inputs at once"
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
        planned_platform: PlannedPlatform,
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
            planned_platform,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "entry identity needs all eleven inputs at once"
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
        lane_variant: Option<LaneVariant>,
        planned_platform: PlannedPlatform,
    ) -> Result<Self, ContractError> {
        let (id, matrix_key) = matrix_identity_for_lane(stack_id, task_group_id, lane_variant)?;
        validate_run_key(run_key)?;
        validate_digest(input_digest)?;
        super::plan::validate_matrix_run(run)?;
        validate_digest(task_digest)?;
        execute_task_ids.validate()?;
        crate::workflow::validate_job_id(job_id)?;
        planned_platform.validate()?;
        Ok(Self {
            report_id: report_id_for_matrix(run_key, &matrix_key)?,
            artifact_id: artifact_id_for_crate_job(run_key, job_id)?,
            id,
            matrix_key,
            job_id: job_id.to_owned(),
            stack_id: stack_id.to_owned(),
            task_id: task_group_id.to_owned(),
            lane_variant,
            planned_platform,
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
            if cache_ids.platform_id() != self.planned_platform.platform_id {
                return Err(ContractError::identity(
                    "cache_ids.platform_id",
                    "planned_platform_mismatch",
                ));
            }
        }
        self.planned_platform.validate()?;
        crate::workflow::validate_job_id(&self.job_id)?;
        let (expect_id, _) =
            matrix_identity_for_lane(&self.stack_id, &self.task_id, self.lane_variant)?;
        if expect_id != self.id {
            return Err(ContractError::identity("id", "id_mismatch"));
        }
        Ok(())
    }
}

pub(super) fn matrix_identity_for_lane(
    stack_id: &str,
    task_id: &str,
    variant: Option<LaneVariant>,
) -> Result<(String, String), ContractError> {
    let group = lane_task_group_id(task_id, variant)?;
    let id = matrix_id_for_task_group(stack_id, &group)?;
    let key = matrix_key_for_id(&id)?;
    Ok((id, key))
}

#[must_use]
pub fn task_matrix_identity_matches(
    task_id: &str,
    matrix_id: &str,
    matrix_key: &str,
    variant: Option<LaneVariant>,
) -> bool {
    VelnorConfig::REGISTERED_STACKS.iter().any(|stack| {
        matrix_identity_for_lane(stack, task_id, variant)
            .is_ok_and(|(id, key)| id == matrix_id && key == matrix_key)
    })
}

pub(super) fn project_task_matrix_identity(
    task_id: &str,
    matrix_id: &str,
    matrix_key: &str,
    variant: LaneVariant,
) -> Result<(String, String), ContractError> {
    let mut matches = VelnorConfig::REGISTERED_STACKS.iter().filter(|stack| {
        matrix_identity_for_lane(stack, task_id, None)
            .is_ok_and(|(id, key)| id == matrix_id && key == matrix_key)
    });
    let stack = matches
        .next()
        .ok_or_else(|| ContractError::identity("matrix_id", "task_identity_mismatch"))?;
    if matches.next().is_some() {
        return Err(ContractError::identity(
            "matrix_id",
            "ambiguous_task_identity",
        ));
    }
    matrix_identity_for_lane(stack, task_id, Some(variant))
}

fn lane_task_group_id(
    task_group_id: &str,
    variant: Option<LaneVariant>,
) -> Result<String, ContractError> {
    crate::ids::validate_task_id(task_group_id)?;
    let Some(variant) = variant else {
        return Ok(task_group_id.to_owned());
    };
    let suffix = match variant {
        LaneVariant::Hosted => "hosted",
        LaneVariant::ScaleSet => "scale-set",
    };
    Ok(format!("{task_group_id}/lane-{suffix}"))
}

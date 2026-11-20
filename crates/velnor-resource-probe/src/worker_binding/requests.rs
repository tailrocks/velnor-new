// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use super::{WorkerRole, validate_container_id, validate_operation_id};
use crate::ProbeError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfStorageRequest {
    pub schema_version: u8,

    pub operation_id: String,

    pub container_id: String,

    pub role: WorkerRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedCgroupRequest {
    pub schema_version: u8,

    pub operation_id: String,

    pub runner_container_id: String,

    pub runner_init_pid: u32,

    pub runner_init_start_ticks: u64,

    pub runner_helper_pid: u32,

    pub runner_helper_start_ticks: u64,

    pub dind_container_id: String,

    pub dind_init_pid: u32,

    pub dind_init_start_ticks: u64,

    pub dind_helper_pid: u32,

    pub dind_helper_start_ticks: u64,

    pub docker_root_path: String,
}

impl SelfStorageRequest {
    pub fn validate(&self) -> Result<(), ProbeError> {
        if self.schema_version != 1 {
            return Err(ProbeError::Invalid("worker_schema_version"));
        }
        validate_operation_id(&self.operation_id)?;
        validate_container_id(&self.container_id)
    }
}

impl OwnedCgroupRequest {
    pub fn validate(&self) -> Result<(), ProbeError> {
        if self.schema_version != 2 {
            return Err(ProbeError::Invalid("worker_schema_version"));
        }
        validate_operation_id(&self.operation_id)?;
        validate_container_id(&self.runner_container_id)?;
        validate_container_id(&self.dind_container_id)?;
        if self.runner_container_id == self.dind_container_id
            || self.runner_init_pid == 0
            || self.runner_helper_pid == 0
            || self.dind_init_pid == 0
            || self.dind_helper_pid == 0
            || self.runner_init_pid == self.runner_helper_pid
            || self.dind_init_pid == self.dind_helper_pid
            || self.runner_init_start_ticks == 0
            || self.runner_helper_start_ticks == 0
            || self.dind_init_start_ticks == 0
            || self.dind_helper_start_ticks == 0
        {
            return Err(ProbeError::Invalid("owned_process_identity"));
        }
        crate::worker_binding::observer::root_backing::validate_docker_root_path(
            &self.docker_root_path,
        )?;
        let process_ids = [
            self.runner_init_pid,
            self.runner_helper_pid,
            self.dind_init_pid,
            self.dind_helper_pid,
        ];
        if process_ids
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != process_ids.len()
        {
            return Err(ProbeError::Invalid("owned_process_duplicate"));
        }
        Ok(())
    }
}

//! Worker request records and checks.

use serde::{Deserialize, Serialize};

use super::{WorkerRole, validate_container_id, validate_operation_id};
use crate::ProbeError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Request for one worker storage observation.
pub struct SelfStorageRequest {
    /// Wire schema version. Must equal one.
    pub schema_version: u8,
    /// Fresh 32-character lower-case hexadecimal identity.
    pub operation_id: String,
    /// Exact container identity.
    pub container_id: String,
    /// Fixed role for this observation.
    pub role: WorkerRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Request for both owned worker cgroup chains.
pub struct OwnedCgroupRequest {
    /// Wire schema version. Must equal two.
    pub schema_version: u8,
    /// Fresh 32-character lower-case hexadecimal identity.
    pub operation_id: String,
    /// Runner container identity.
    pub runner_container_id: String,
    /// Runner init PID from Docker inspect.
    pub runner_init_pid: u32,
    /// Expected runner init process start time.
    pub runner_init_start_ticks: u64,
    /// Runner helper PID from Docker exec inspect.
    pub runner_helper_pid: u32,
    /// Expected runner helper process start time.
    pub runner_helper_start_ticks: u64,
    /// `DinD` container identity.
    pub dind_container_id: String,
    /// `DinD` init PID from Docker inspect.
    pub dind_init_pid: u32,
    /// Expected `DinD` init process start time.
    pub dind_init_start_ticks: u64,
    /// `DinD` helper PID from Docker exec inspect.
    pub dind_helper_pid: u32,
    /// Expected `DinD` helper process start time.
    pub dind_helper_start_ticks: u64,
    /// Selected Docker root directory from the verified engine.
    pub docker_root_path: String,
}

impl SelfStorageRequest {
    /// Validate schema version and identities.
    ///
    /// # Errors
    ///
    /// Returns an error when the schema or either identity is invalid.
    pub fn validate(&self) -> Result<(), ProbeError> {
        if self.schema_version != 1 {
            return Err(ProbeError::Invalid("worker_schema_version"));
        }
        validate_operation_id(&self.operation_id)?;
        validate_container_id(&self.container_id)
    }
}

impl OwnedCgroupRequest {
    /// Validate schema version and owned process identities.
    ///
    /// # Errors
    ///
    /// Returns an error when any container or process identity is invalid.
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

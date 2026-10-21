//! One purpose-bound measurement of the current worker cgroups.

use sha2::{Digest, Sha256};
use velnor_resource_probe::worker_binding::{
    OwnedCgroupObservation, OwnedCgroupRequest, WorkerRole,
};

use crate::error::HostError;

/// Process facts from the exact running worker containers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedContainerSubjects {
    container_id: String,
    init_host_pid: u32,
    init_start_ticks: u64,
    helper_host_pid: u32,
    helper_start_ticks: u64,
}

impl OwnedContainerSubjects {
    /// Bind one container to its live init and self-storage helper processes.
    pub(crate) fn new(
        container_id: String,
        init_host_pid: u32,
        init_start_ticks: u64,
        helper_host_pid: u32,
        helper_start_ticks: u64,
    ) -> Result<Self, HostError> {
        let value = Self {
            container_id,
            init_host_pid,
            init_start_ticks,
            helper_host_pid,
            helper_start_ticks,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), HostError> {
        if !valid_container_id(&self.container_id)
            || self.init_host_pid == 0
            || self.helper_host_pid == 0
            || self.init_host_pid == self.helper_host_pid
            || self.init_start_ticks == 0
            || self.helper_start_ticks == 0
        {
            return Err(HostError::Identity);
        }
        Ok(())
    }
}

/// The exact Runner and `DinD` process identities for one observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedCgroupSubjects {
    runner: OwnedContainerSubjects,
    dind: OwnedContainerSubjects,
}

impl OwnedCgroupSubjects {
    /// Bind the two distinct worker containers in protocol order.
    pub(crate) fn new(
        runner: OwnedContainerSubjects,
        dind: OwnedContainerSubjects,
    ) -> Result<Self, HostError> {
        let value = Self { runner, dind };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), HostError> {
        self.runner.validate()?;
        self.dind.validate()?;
        let ids = [&self.runner.container_id, &self.dind.container_id];
        let pids = [
            self.runner.init_host_pid,
            self.runner.helper_host_pid,
            self.dind.init_host_pid,
            self.dind.helper_host_pid,
        ];
        if ids[0] == ids[1]
            || pids
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != pids.len()
        {
            return Err(HostError::Identity);
        }
        Ok(())
    }

    pub(super) fn request(
        &self,
        operation_id: String,
        docker_root_path: String,
    ) -> OwnedCgroupRequest {
        OwnedCgroupRequest {
            schema_version: 2,
            operation_id,
            runner_container_id: self.runner.container_id.clone(),
            runner_init_pid: self.runner.init_host_pid,
            runner_init_start_ticks: self.runner.init_start_ticks,
            runner_helper_pid: self.runner.helper_host_pid,
            runner_helper_start_ticks: self.runner.helper_start_ticks,
            dind_container_id: self.dind.container_id.clone(),
            dind_init_pid: self.dind.init_host_pid,
            dind_init_start_ticks: self.dind.init_start_ticks,
            dind_helper_pid: self.dind.helper_host_pid,
            dind_helper_start_ticks: self.dind.helper_start_ticks,
            docker_root_path,
        }
    }

    pub(super) fn request_line(
        &self,
        operation_id: &str,
        docker_root_path: &str,
    ) -> Result<Vec<u8>, HostError> {
        let mut bytes = self
            .request_json(operation_id, docker_root_path)?
            .into_bytes();
        bytes.push(b'\n');
        if bytes.len() > velnor_resource_probe::worker_binding::WORKER_REQUEST_LIMIT {
            return Err(HostError::Frame);
        }
        Ok(bytes)
    }

    pub(super) fn request_json(
        &self,
        operation_id: &str,
        docker_root_path: &str,
    ) -> Result<String, HostError> {
        let request = self.request(operation_id.to_owned(), docker_root_path.to_owned());
        request.validate().map_err(|_| HostError::Identity)?;
        serde_json::to_string(&request).map_err(|_| HostError::Frame)
    }

    pub(super) fn request_sha256(
        &self,
        operation_id: &str,
        docker_root_path: &str,
    ) -> Result<String, HostError> {
        let bytes = self.request_line(operation_id, docker_root_path)?;
        let mut digest = Sha256::new();
        digest.update(b"velnor-owned-cgroups-request-v1\0");
        digest.update(bytes);
        Ok(crate::digest_hex::lowercase_hex(&digest.finalize()))
    }
}

/// One cgroup measurement returned after its probe operation is retired.
pub(crate) struct VerifiedOwnedCgroups {
    operation_id: String,
    engine_id: String,
    docker_root_digest: String,
    observation: OwnedCgroupObservation,
    probe_host_pid: u32,
    cleanup: super::execute::ProbeCleanupEvidence,
}

impl VerifiedOwnedCgroups {
    pub(super) fn new(
        observation: OwnedCgroupObservation,
        cleanup: super::execute::ProbeCleanupEvidence,
        probe_host_pid: u32,
    ) -> Self {
        Self {
            operation_id: cleanup.operation_id().to_owned(),
            engine_id: cleanup.engine_id().to_owned(),
            docker_root_digest: cleanup.docker_root_digest().to_owned(),
            observation,
            probe_host_pid,
            cleanup,
        }
    }

    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn observation(&self) -> &OwnedCgroupObservation {
        &self.observation
    }

    pub(crate) const fn probe_host_pid(&self) -> u32 {
        self.probe_host_pid
    }

    pub(crate) fn cleanup(&self) -> &super::execute::ProbeCleanupEvidence {
        &self.cleanup
    }
}

pub(super) fn validate_observation(
    observation: &OwnedCgroupObservation,
    request: &OwnedCgroupRequest,
) -> Result<(), HostError> {
    request.validate().map_err(|_| HostError::Identity)?;
    let [runner, dind] = observation.containers.as_slice() else {
        return Err(HostError::Identity);
    };
    let [runner_root, dind_root] = observation.root_backings.as_slice() else {
        return Err(HostError::Identity);
    };
    if observation.wire_mode != "owned_container_cgroup"
        || observation.schema_version != OwnedCgroupObservation::SCHEMA_VERSION
        || observation.operation_id != request.operation_id
        || observation.guest_boot_id.is_empty()
        || runner.role != WorkerRole::Runner
        || runner.container_id != request.runner_container_id
        || runner.init.host_pid != request.runner_init_pid
        || runner.init.start_ticks != request.runner_init_start_ticks
        || runner.helper.host_pid != request.runner_helper_pid
        || runner.helper.start_ticks != request.runner_helper_start_ticks
        || dind.role != WorkerRole::Dind
        || dind.container_id != request.dind_container_id
        || dind.init.host_pid != request.dind_init_pid
        || dind.init.start_ticks != request.dind_init_start_ticks
        || dind.helper.host_pid != request.dind_helper_pid
        || dind.helper.start_ticks != request.dind_helper_start_ticks
        || runner_root.role != WorkerRole::Runner
        || runner_root.container_id != request.runner_container_id
        || runner_root.guest_boot_id != observation.guest_boot_id
        || runner_root.init != runner.init
        || runner_root.helper != runner.helper
        || dind_root.role != WorkerRole::Dind
        || dind_root.container_id != request.dind_container_id
        || dind_root.guest_boot_id != observation.guest_boot_id
        || dind_root.init != dind.init
        || dind_root.helper != dind.helper
        || observation.engine_metadata.role
            != velnor_resource_probe::worker_binding::AttachmentRole::EngineMetadata
        || observation.engine_metadata.target != "/velnor/docker-root"
        || !observation.engine_metadata.read_only
        || observation.engine_metadata.mount_id == 0
        || observation.engine_metadata.statx_mount_id == 0
        || observation.engine_metadata.filesystem_id == 0
        || observation.engine_metadata.mount_source.is_empty()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn parse_record(
    bytes: &[u8],
    request: &OwnedCgroupRequest,
) -> Result<OwnedCgroupObservation, HostError> {
    let limit = velnor_resource_probe::worker_binding::WORKER_RECORD_LIMIT;
    if bytes.len() < 2
        || bytes.len() > limit
        || bytes.last() != Some(&b'\n')
        || bytes[..bytes.len() - 1].contains(&b'\n')
        || bytes
            .iter()
            .any(|byte| byte.is_ascii_control() && *byte != b'\n')
    {
        return Err(HostError::Frame);
    }
    let body = &bytes[..bytes.len() - 1];
    let observation: OwnedCgroupObservation =
        serde_json::from_slice(body).map_err(|_| HostError::Frame)?;
    if serde_json::to_vec(&observation).map_err(|_| HostError::Frame)? != body {
        return Err(HostError::Frame);
    }
    validate_observation(&observation, request)?;
    Ok(observation)
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "owned_cgroups/tests.rs"]
mod tests;

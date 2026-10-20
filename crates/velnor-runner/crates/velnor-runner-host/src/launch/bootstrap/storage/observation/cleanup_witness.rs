use std::time::Instant;

use bollard::Docker;
use velnor_resource_probe::{
    cleanup_observer_wire::{CleanupDockerRootIdentity, ProcessIdentity},
    worker_binding::{OwnedCgroupObservation, ProcessWitness, WorkerRole},
};

#[path = "cleanup_witness/volumes.rs"]
mod volumes;

pub(super) use volumes::capture_cleanup_binding;

use crate::{error::HostError, journal::EngineContext, launch::SelectedEngineContext};

fn container_witness(
    owned: &OwnedCgroupObservation,
    role: WorkerRole,
) -> Result<&velnor_resource_probe::worker_binding::OwnedContainerCgroupObservation, HostError> {
    let mut matches = owned.containers.iter().filter(|row| row.role == role);
    let row = matches.next().ok_or(HostError::Identity)?;
    if matches.next().is_some() {
        return Err(HostError::Identity);
    }
    Ok(row)
}

fn process_identity(witness: &ProcessWitness) -> Result<ProcessIdentity, HostError> {
    let value = ProcessIdentity {
        pid: witness.host_pid,
        start_ticks: witness.start_ticks,
        namespace_device: [
            witness.mount_namespace_device_major,
            witness.mount_namespace_device_minor,
        ],
        namespace_inode: witness.mount_namespace_id,
    };
    (value.pid > 0 && value.start_ticks > 0 && value.namespace_inode > 0)
        .then_some(value)
        .ok_or(HostError::Identity)
}

fn docker_root_identity(
    owned: &OwnedCgroupObservation,
) -> Result<CleanupDockerRootIdentity, HostError> {
    let row = &owned.engine_metadata;
    if row.target != "/velnor/docker-root"
        || !row.read_only
        || row.mount_id == 0
        || row.mount_id != row.statx_mount_id
        || row.filesystem_id == 0
        || row.statx_inode == 0
    {
        return Err(HostError::Identity);
    }
    Ok(CleanupDockerRootIdentity {
        target: row.target.clone(),
        mount_id: row.mount_id,
        statx_mount_id: row.statx_mount_id,
        mount_root: row.mount_root.clone(),
        mount_device: [row.mount_device_major, row.mount_device_minor],
        statx_device: [row.statx_device_major, row.statx_device_minor],
        filesystem_type_magic: row.filesystem_type_magic,
        filesystem_id: row.filesystem_id,
        read_only: row.read_only,
    })
}

async fn verify_engine(
    docker: &Docker,
    selected: &SelectedEngineContext,
    deadline: Instant,
) -> Result<(), HostError> {
    let context = EngineContext {
        engine_id: selected.engine_id().to_owned(),
        endpoint: selected.endpoint().to_owned(),
        platform: selected.platform().to_owned(),
    };
    let info = within(deadline, docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let current = SelectedEngineContext::capture(&context, &info)?;
    if current.engine_id() == selected.engine_id()
        && current.docker_root_digest() == selected.docker_root_digest()
    {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

async fn within<T>(
    deadline: Instant,
    future: impl std::future::Future<Output = T>,
) -> Result<T, HostError> {
    tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), future)
        .await
        .map_err(|_| HostError::DockerTimeout)
}

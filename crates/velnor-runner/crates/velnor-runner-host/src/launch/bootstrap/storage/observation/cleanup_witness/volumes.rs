use std::time::Instant;

use bollard::{Docker, errors::Error as DockerError};
use velnor_resource_probe::{
    cleanup_observer_wire::{CleanupObserverBinding, CleanupVolume, validate_binding},
    worker_binding::{OwnedCgroupObservation, WorkerRole},
};

use crate::{
    error::HostError,
    journal::{ConnectionJournal, StorageProvisioningPlan, StorageRole},
    launch::{SelectedEngineContext, VerifiedWorkerImages, bootstrap::PreparedAcquiredBootstrap},
    worker::storage_initialization::VerifiedStorageInitializationReady,
};

use super::{container_witness, docker_root_identity, process_identity, verify_engine, within};

mod volume_identity;
use volume_identity::{
    object_for_role, tmpfs_devices, validate_object_identity, verify_volume, volume_identity,
};

pub(super) const VOLUME_ROLES: [StorageRole; 5] = [
    StorageRole::SharedRun,
    StorageRole::RunnerHome,
    StorageRole::RunnerWork,
    StorageRole::DindData,
    StorageRole::EngineMetadata,
];

/// This function reads all worker and volume identity data while both workers are running.
pub(in crate::launch::bootstrap::storage::observation) async fn capture_cleanup_binding(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    prepared: &PreparedAcquiredBootstrap,
    runner_id: &str,
    dind_id: &str,
    plan: &StorageProvisioningPlan,
    owned: &OwnedCgroupObservation,
    attachments: &[super::super::worker::MeasuredAttachment],
    initialization: &VerifiedStorageInitializationReady,
    deadline: Instant,
) -> Result<CleanupObserverBinding, HostError> {
    if owned.guest_boot_id.is_empty()
        || owned.daemon.host_pid == 0
        || owned.containers.len() != 2
        || !engine.matches_images(images)
        || plan.reservation_id() != prepared.acquired().reservation_id()
        || plan.operation_id() != prepared.acquired().intent_id()
        || initialization.reservation_id() != plan.reservation_id()
        || initialization.operation_id() != plan.operation_id()
        || initialization.engine_id() != engine.engine_id()
        || initialization.docker_root_digest() != engine.docker_root_digest()
        || initialization.metadata_volume_name() != prepared.metadata().docker_name()
        || initialization.metadata_volume_id() != prepared.metadata().docker_id()
    {
        return Err(HostError::Identity);
    }

    within(deadline, verify_engine(docker, engine, deadline)).await??;
    within(deadline, images.verify_for_engine(docker, engine)).await??;
    let mut volumes = Vec::with_capacity(VOLUME_ROLES.len());
    for role in VOLUME_ROLES {
        let object = object_for_role(prepared, role)?;
        validate_object_identity(object, plan, engine)?;
        let inspected = within(deadline, docker.inspect_volume(object.docker_name()))
            .await?
            .map_err(|error| match error {
                DockerError::DockerResponseServerError {
                    status_code: 404, ..
                } => HostError::Ownership,
                _ => HostError::Docker,
            })?;
        verify_volume(&inspected, object.docker_name(), role, journal, plan)?;
        let mountpoint = inspected.mountpoint.clone();
        if mountpoint.is_empty() || mountpoint != mountpoint.trim() {
            return Err(HostError::Identity);
        }
        let identity = volume_identity(attachments, role, runner_id, dind_id, initialization)?;
        volumes.push(CleanupVolume {
            role: role.as_str().to_owned(),
            mountpoint,
            mount_device: identity.mount_device,
            statx_device: identity.statx_device,
            inode: identity.inode,
        });
    }
    within(deadline, verify_engine(docker, engine, deadline)).await??;

    let runner = container_witness(owned, WorkerRole::Runner)?;
    let dind = container_witness(owned, WorkerRole::Dind)?;
    let binding = CleanupObserverBinding {
        expected_guest_boot_id: owned.guest_boot_id.clone(),
        metadata_job_facts_inode: if plan
            .roles()
            .iter()
            .any(|role| role.role() == StorageRole::EngineMetadata)
        {
            Some(initialization.job_facts().inode())
        } else {
            None
        },
        docker_root: docker_root_identity(owned)?,
        daemon: process_identity(&owned.daemon)?,
        runner: process_identity(&runner.init)?,
        dind: process_identity(&dind.init)?,
        cgroup_paths: [
            runner.init.cgroup_path.clone(),
            dind.init.cgroup_path.clone(),
        ],
        volume_mountpoints: volumes,
        owned_tmpfs_devices: tmpfs_devices(attachments, runner_id, dind_id)?,
    };
    validate_binding(&binding).map_err(|_| HostError::Identity)?;
    Ok(binding)
}

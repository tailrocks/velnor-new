use crate::error::HostError;
use crate::journal::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
};
use crate::launch::bootstrap::{
    BootstrapContainerRole, ObservedContainerCgroup, ObservedStorageAttachment, ObservedStorageRole,
};

use crate::launch::bootstrap::test_fixtures::valid_digest;

#[path = "storage/observed.rs"]
mod observed;
pub(crate) use observed::{
    test_observed_container_cgroup, test_observed_storage_attachment, test_observed_storage_role,
};

pub(crate) fn test_verified_storage_resource_binding(
    input: super::super::VerifiedStorageResourceBindingFixture,
) -> Result<super::VerifiedStorageResourceBinding, HostError> {
    let cleanup_observer_binding = cleanup_binding(
        &input.guest_boot_id,
        &input.runner_container_id,
        &input.dind_container_id,
        &input.attachments,
        &input.cgroups,
    )?;
    let root_backings = root_backings_from_fixture(&input)?;
    let super::super::VerifiedStorageResourceBindingFixture {
        reservation_id,
        operation_id,
        engine_id,
        docker_root_digest,
        sample_generation_id,
        profile_generation_id,
        profile_digest,
        runner_container_id,
        dind_container_id,
        connection_id: _,
        instance_binding_digest,
        roles,
        attachments,
        cgroups,
        guest_boot_id,
    } = input;
    let binding = super::VerifiedStorageResourceBinding {
        reservation_id,
        operation_id,
        engine_id,
        docker_root_digest,
        sample_generation_id,
        profile_generation_id,
        profile_digest,
        runner_container_id,
        dind_container_id,
        instance_binding_digest,
        roles,
        attachments,
        cgroups,
        root_backings,
        guest_boot_id,
        cleanup_observer_binding,
    };
    super::validation::validate_storage_resource_binding(&binding)?;
    Ok(binding)
}

fn root_backings_from_fixture(
    input: &super::super::VerifiedStorageResourceBindingFixture,
) -> Result<Vec<super::VerifiedRootBackingMapping>, HostError> {
    let root_backings = [
        (
            StorageRole::RunnerRoot,
            StorageObserver::Runner,
            &input.runner_container_id,
        ),
        (
            StorageRole::DindRoot,
            StorageObserver::Dind,
            &input.dind_container_id,
        ),
    ]
    .into_iter()
    .map(|(role, observer, container_id)| {
        let attachment = input
            .attachments
            .iter()
            .find(|attachment| {
                attachment.role() == role
                    && attachment.observer() == observer
                    && attachment.target() == "/"
                    && attachment.container_id() == Some(container_id.as_str())
            })
            .ok_or(HostError::Journal)?;
        let capacity_digest = input
            .roles
            .iter()
            .find(|observed| observed.role() == role)
            .ok_or(HostError::Journal)?
            .filesystem_instance_digest();
        Ok(super::VerifiedRootBackingMapping::test_fixture(
            crate::launch::bootstrap::RootBackingMappingFixtureInput {
                reservation_id: input.reservation_id,
                operation_id: input.operation_id,
                connection_id: &input.connection_id,
                engine_id: &input.engine_id,
                docker_root_digest: &input.docker_root_digest,
                sample_generation_id: &input.sample_generation_id,
                role,
                container_id,
                image_runtime_id: if observer == StorageObserver::Runner {
                    "runner-image-runtime"
                } else {
                    "dind-image-runtime"
                },
                image_source_sha: &"a".repeat(40),
                attachment,
                backing_filesystem_instance_digest: capacity_digest,
            },
        ))
    })
    .collect::<Result<Vec<_>, HostError>>()?;
    Ok(root_backings)
}

fn cleanup_binding(
    boot_id: &str,
    runner_id: &str,
    dind_id: &str,
    attachments: &[ObservedStorageAttachment],
    cgroups: &[ObservedContainerCgroup],
) -> Result<velnor_resource_probe::cleanup_observer_wire::CleanupObserverBinding, HostError> {
    use velnor_resource_probe::cleanup_observer_wire::{
        CleanupDockerRootIdentity, CleanupObserverBinding, ProcessIdentity,
    };
    let volumes = cleanup_volumes(attachments)?;
    let owned_tmpfs_devices = cleanup_tmpfs_devices(attachments)?;
    let metadata = cleanup_root(
        attachments,
        StorageRole::EngineMetadata,
        StorageObserver::Probe,
        None,
        "/velnor/docker-root",
    )?;
    let job_facts = cleanup_root(
        attachments,
        StorageRole::EngineMetadata,
        StorageObserver::Runner,
        Some(runner_id),
        "/opt/velnor/job-facts",
    )?;
    let dind_job_facts = cleanup_root(
        attachments,
        StorageRole::EngineMetadata,
        StorageObserver::Dind,
        Some(dind_id),
        "/opt/velnor/job-facts",
    )?;
    if job_facts.statx_inode() == 0 || job_facts.statx_inode() != dind_job_facts.statx_inode() {
        return Err(HostError::Identity);
    }
    let paths = [
        cleanup_cgroup_path(cgroups, BootstrapContainerRole::Runner)?,
        cleanup_cgroup_path(cgroups, BootstrapContainerRole::Dind)?,
    ];
    let process = |pid, inode| ProcessIdentity {
        pid,
        start_ticks: u64::from(pid) * 100,
        namespace_device: [0, 6],
        namespace_inode: inode,
    };
    Ok(CleanupObserverBinding {
        expected_guest_boot_id: boot_id.to_owned(),
        metadata_job_facts_inode: Some(job_facts.statx_inode()),
        docker_root: CleanupDockerRootIdentity {
            target: "/velnor/docker-root".to_owned(),
            mount_id: metadata.mount_id(),
            statx_mount_id: metadata.mount_id(),
            mount_root: metadata.mount_root().to_owned(),
            mount_device: [metadata.device().0, metadata.device().1],
            statx_device: [metadata.statx_device().0, metadata.statx_device().1],
            filesystem_type_magic: metadata.fs_type_magic(),
            filesystem_id: metadata.filesystem_id(),
            read_only: metadata.read_only(),
        },
        daemon: process(1, 101),
        runner: process(2, 102),
        dind: process(3, 103),
        cgroup_paths: paths,
        volume_mountpoints: volumes,
        owned_tmpfs_devices,
    })
}

fn cleanup_volumes(
    attachments: &[ObservedStorageAttachment],
) -> Result<Vec<velnor_resource_probe::cleanup_observer_wire::CleanupVolume>, HostError> {
    use velnor_resource_probe::cleanup_observer_wire::CleanupVolume;
    let roles = [
        (StorageRole::SharedRun, StorageObserver::Runner, "/run"),
        (
            StorageRole::RunnerHome,
            StorageObserver::Runner,
            "/home/runner",
        ),
        (
            StorageRole::RunnerWork,
            StorageObserver::Runner,
            "/home/runner/_work",
        ),
        (
            StorageRole::DindData,
            StorageObserver::Dind,
            "/var/lib/docker",
        ),
        (
            StorageRole::EngineMetadata,
            StorageObserver::Runner,
            "/opt/velnor/job-facts",
        ),
    ];
    let names = [
        "shared-run",
        "runner-home",
        "runner-work",
        "dind-data",
        "engine-metadata",
    ];
    let mut volumes = Vec::with_capacity(names.len());
    for ((role, observer, target), name) in roles.into_iter().zip(names) {
        let actual_container = attachments
            .iter()
            .find(|row| row.role() == role && row.observer() == observer && row.target() == target)
            .and_then(ObservedStorageAttachment::container_id);
        if (observer == StorageObserver::Probe) != actual_container.is_none() {
            return Err(HostError::Journal);
        }
        let row = cleanup_root(attachments, role, observer, actual_container, target)?;
        volumes.push(CleanupVolume {
            role: name.to_owned(),
            mountpoint: format!("/var/lib/docker/volumes/test-{name}/_data"),
            mount_device: [row.device().0, row.device().1],
            statx_device: [row.statx_device().0, row.statx_device().1],
            inode: if role == StorageRole::EngineMetadata {
                row.statx_inode().saturating_add(1)
            } else {
                row.statx_inode()
            },
        });
    }
    Ok(volumes)
}

fn cleanup_tmpfs_devices(
    attachments: &[ObservedStorageAttachment],
) -> Result<[[u32; 2]; 8], HostError> {
    let tmpfs = [
        (StorageRole::SharedRun, StorageObserver::Runner, "/run"),
        (
            StorageRole::RunnerHome,
            StorageObserver::Runner,
            "/home/runner",
        ),
        (
            StorageRole::RunnerWork,
            StorageObserver::Runner,
            "/home/runner/_work",
        ),
        (StorageRole::RunnerTmp, StorageObserver::Runner, "/tmp"),
        (StorageRole::RunnerShm, StorageObserver::Runner, "/dev/shm"),
        (
            StorageRole::DindData,
            StorageObserver::Dind,
            "/var/lib/docker",
        ),
        (StorageRole::DindTmp, StorageObserver::Dind, "/tmp"),
        (StorageRole::DindShm, StorageObserver::Dind, "/dev/shm"),
    ];
    let mut devices = [[0; 2]; 8];
    for (index, (role, observer, target)) in tmpfs.into_iter().enumerate() {
        let container = attachments
            .iter()
            .find(|row| row.role() == role && row.observer() == observer && row.target() == target)
            .and_then(ObservedStorageAttachment::container_id);
        let row = cleanup_root(attachments, role, observer, container, target)?;
        devices[index] = [row.device().0, row.device().1];
    }
    Ok(devices)
}

fn cleanup_root<'a>(
    attachments: &'a [ObservedStorageAttachment],
    role: StorageRole,
    observer: StorageObserver,
    container: Option<&str>,
    target: &str,
) -> Result<&'a ObservedStorageAttachment, HostError> {
    attachments
        .iter()
        .find(|row| {
            row.role() == role
                && row.observer() == observer
                && row.container_id() == container
                && row.target() == target
        })
        .ok_or(HostError::Journal)
}

fn cleanup_cgroup_path(
    cgroups: &[ObservedContainerCgroup],
    role: BootstrapContainerRole,
) -> Result<String, HostError> {
    cgroups
        .iter()
        .find(|row| row.role() == role)
        .map(|row| row.path().to_owned())
        .ok_or(HostError::Journal)
}

fn validate_storage_role(value: &ObservedStorageRole) -> Result<(), HostError> {
    let contract = value.role.contract().ok_or(HostError::Journal)?;
    let (total, free, available) = value.byte_counts();
    let (inode_total, inode_free, inode_available) = value.inode_counts();
    let (charge_bytes, charge_inodes, retained_memory) = value.charges();
    let (owned_bytes, owned_inodes) = value.owned_usage();
    let bytes_cover_charge = available
        .checked_add(owned_bytes)
        .is_some_and(|remaining| remaining >= charge_bytes);
    let inodes_cover_charge = match value.inode_policy() {
        StorageInodePolicy::Finite => inode_available
            .and_then(|available| available.checked_add(owned_inodes))
            .is_some_and(|remaining| remaining >= charge_inodes),
        StorageInodePolicy::DynamicBtrfs => {
            charge_inodes > 0
                && owned_inodes == 0
                && inode_total == Some(0)
                && inode_free == Some(0)
                && inode_available == Some(0)
        }
        StorageInodePolicy::Unknown => false,
    };
    let filesystem_policy_matches = match value.medium() {
        StorageMedium::Tmpfs => {
            value.filesystem_kind() == StorageFilesystemKind::Tmpfs
                && value.inode_policy() == StorageInodePolicy::Finite
                && retained_memory == charge_bytes
        }
        StorageMedium::Disk => {
            value.filesystem_kind() != StorageFilesystemKind::Tmpfs && retained_memory == 0
        }
    };
    let inode_shape_matches = match value.inode_policy() {
        StorageInodePolicy::Finite => match (inode_total, inode_free, inode_available) {
            (Some(total), Some(free), Some(available)) => {
                total > 0 && free <= total && available <= free && charge_inodes > 0
            }
            _ => false,
        },
        StorageInodePolicy::DynamicBtrfs => value.filesystem_kind() == StorageFilesystemKind::Btrfs,
        StorageInodePolicy::Unknown => false,
    };
    if !valid_digest(value.pool_id())
        || !valid_digest(value.filesystem_instance_digest())
        || value.medium() != contract.medium
        || total == 0
        || free > total
        || available > free
        || charge_bytes == 0
        || owned_bytes > charge_bytes
        || !bytes_cover_charge
        || !inodes_cover_charge
        || !filesystem_policy_matches
        || !inode_shape_matches
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

#[cfg(test)]
#[path = "storage/tests.rs"]
mod tests;

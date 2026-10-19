use crate::error::HostError;
use crate::journal::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
};
use crate::launch::bootstrap::{
    BootstrapContainerRole, ObservedContainerCgroup, ObservedStorageAttachment, ObservedStorageRole,
};

use crate::launch::bootstrap::test_fixtures::{
    valid_bounded_text, valid_container_id, valid_digest,
};

pub(crate) fn test_observed_storage_role(
    role: StorageRole,
    pool_id: String,
    filesystem_instance_digest: String,
    medium: StorageMedium,
    filesystem_kind: StorageFilesystemKind,
    byte_counts: (u64, u64, u64),
    inode_counts: (Option<u64>, Option<u64>, Option<u64>),
    inode_policy: StorageInodePolicy,
    charges: (u64, u64, u64),
    owned_usage: (u64, u64),
) -> Result<ObservedStorageRole, HostError> {
    let value = ObservedStorageRole {
        role,
        pool_id,
        filesystem_instance_digest,
        medium,
        filesystem_kind,
        total_bytes: byte_counts.0,
        free_bytes: byte_counts.1,
        available_bytes: byte_counts.2,
        inode_total: inode_counts.0,
        inode_free: inode_counts.1,
        inode_available: inode_counts.2,
        inode_policy,
        charge_bytes: charges.0,
        charge_inodes: charges.1,
        retained_memory_bytes: charges.2,
        owned_usage_bytes: owned_usage.0,
        owned_usage_inodes: owned_usage.1,
    };
    validate_storage_role(&value)?;
    Ok(value)
}

pub(crate) fn test_observed_storage_attachment(
    role: StorageRole,
    observer: StorageObserver,
    container_id: Option<String>,
    target: String,
    source_target: Option<String>,
    read_only: bool,
    mount_id: u64,
    device: (u32, u32),
    statx_device: (u32, u32),
    fs_type_magic: u64,
    filesystem_id: u64,
    filesystem_instance_digest: String,
) -> Result<ObservedStorageAttachment, HostError> {
    let contract = role
        .attachments()
        .iter()
        .find(|contract| contract.observer == observer && contract.target == target)
        .ok_or(HostError::Journal)?;
    let container_valid = match observer {
        StorageObserver::Runner | StorageObserver::Dind => {
            container_id.as_deref().is_some_and(valid_container_id)
        }
        StorageObserver::Probe => container_id.is_none(),
    };
    if !container_valid
        || source_target.as_deref() != contract.source_target
        || read_only != contract.read_only
        || mount_id == 0
        || !valid_digest(&filesystem_instance_digest)
        || (device == (0, 0) && statx_device == (0, 0))
        || (fs_type_magic == 0)
        || filesystem_id == 0
    {
        return Err(HostError::Journal);
    }
    Ok(ObservedStorageAttachment {
        role,
        observer,
        container_id,
        target,
        source_target,
        read_only,
        mount_id,
        device_major: device.0,
        device_minor: device.1,
        statx_device_major: statx_device.0,
        statx_device_minor: statx_device.1,
        fs_type_magic,
        filesystem_id,
        filesystem_instance_digest,
    })
}

pub(crate) fn test_observed_container_cgroup(
    role: BootstrapContainerRole,
    container_id: String,
    path: String,
    memory: (Option<u64>, u64),
    cpu: (Option<u64>, u64),
    pids_max: Option<u64>,
) -> Result<ObservedContainerCgroup, HostError> {
    if !valid_container_id(&container_id)
        || !valid_bounded_text(&path, 1024)
        || memory.0.is_none_or(|limit| limit == 0 || limit < memory.1)
        || cpu.0.is_none_or(|quota| quota == 0)
        || cpu.1 == 0
        || pids_max.is_none_or(|limit| limit == 0)
    {
        return Err(HostError::Journal);
    }
    Ok(ObservedContainerCgroup {
        role,
        container_id,
        path,
        memory_max_bytes: memory.0,
        memory_current_bytes: memory.1,
        cpu_quota_micros: cpu.0,
        cpu_period_micros: cpu.1,
        pids_max,
    })
}

pub(crate) fn test_verified_storage_resource_binding(
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    profile_generation_id: String,
    profile_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    roles: Vec<ObservedStorageRole>,
    attachments: Vec<ObservedStorageAttachment>,
    cgroups: Vec<ObservedContainerCgroup>,
    guest_boot_id: String,
) -> Result<super::VerifiedStorageResourceBinding, HostError> {
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
        guest_boot_id,
    };
    super::validation::validate_storage_resource_binding(&binding)?;
    Ok(binding)
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
mod tests {
    use super::test_observed_storage_attachment;
    use crate::journal::{StorageObserver, StorageRole};

    const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;

    #[test]
    fn engine_metadata_attachment_binds_exact_docker_root_source() {
        let expected = test_observed_storage_attachment(
            StorageRole::EngineMetadata,
            StorageObserver::Probe,
            None,
            "/velnor/docker-root".to_owned(),
            Some("/var/lib/docker".to_owned()),
            true,
            237,
            (0, 38),
            (0, 41),
            BTRFS_SUPER_MAGIC,
            u64::MAX,
            "a".repeat(64),
        );
        assert!(expected.is_ok_and(|item| item.filesystem_id() == u64::MAX));

        let wrong_source = test_observed_storage_attachment(
            StorageRole::EngineMetadata,
            StorageObserver::Probe,
            None,
            "/velnor/docker-root".to_owned(),
            Some("/tmp/docker".to_owned()),
            true,
            237,
            (0, 38),
            (0, 41),
            BTRFS_SUPER_MAGIC,
            12,
            "a".repeat(64),
        );
        assert!(wrong_source.is_err());
    }

    #[test]
    fn worker_attachment_cannot_name_a_host_source_path() {
        let attachment = test_observed_storage_attachment(
            StorageRole::RunnerRoot,
            StorageObserver::Runner,
            Some("b".repeat(64)),
            "/".to_owned(),
            Some("/var/lib/docker".to_owned()),
            true,
            12,
            (0, 41),
            (0, 41),
            BTRFS_SUPER_MAGIC,
            12,
            "a".repeat(64),
        );
        assert!(attachment.is_err());
    }
}

use crate::error::HostError;
use crate::journal::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
};
use crate::launch::bootstrap::test_fixtures::{
    valid_bounded_text, valid_container_id, valid_digest,
};
use crate::launch::bootstrap::{
    BootstrapContainerRole, ObservedContainerCgroup, ObservedStorageAttachment, ObservedStorageRole,
};

#[expect(
    clippy::too_many_arguments,
    reason = "the storage fixture varies capacity, inode, charge, and ownership facts independently before production validation"
)]
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
    super::validate_storage_role(&value)?;
    Ok(value)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the attachment fixture varies observer, mount, device, and filesystem identity facts independently before validation"
)]
pub(crate) fn test_observed_storage_attachment(
    role: StorageRole,
    observer: StorageObserver,
    container_id: Option<String>,
    target: String,
    source_target: Option<String>,
    mount_root: String,
    read_only: bool,
    mount_id: u64,
    statx_inode: u64,
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
        || !valid_mount_root(&mount_root, contract.volume_subpath)
        || read_only != contract.read_only
        || mount_id == 0
        || statx_inode == 0
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
        volume_subpath: contract.volume_subpath.map(str::to_owned),
        mount_root,
        read_only,
        mount_id,
        statx_inode,
        device_major: device.0,
        device_minor: device.1,
        statx_device_major: statx_device.0,
        statx_device_minor: statx_device.1,
        fs_type_magic,
        filesystem_id,
        filesystem_instance_digest,
    })
}

fn valid_mount_root(value: &str, volume_subpath: Option<&str>) -> bool {
    if value == "/" {
        return volume_subpath.is_none();
    }
    if value.is_empty()
        || value.len() > 1024
        || !value.starts_with('/')
        || value.bytes().any(|byte| byte.is_ascii_control())
        || value
            .split('/')
            .any(|component| component == "." || component == "..")
    {
        return false;
    }
    volume_subpath.is_none_or(|subpath| {
        value
            .strip_suffix(subpath)
            .is_some_and(|prefix| prefix.ends_with('/') && !prefix.is_empty())
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

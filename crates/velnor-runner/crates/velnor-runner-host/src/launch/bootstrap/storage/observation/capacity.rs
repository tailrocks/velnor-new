use super::{super::VerifiedWorkerStorageObservation, worker::MeasuredAttachment};

pub(super) const BTRFS_MAGIC: u64 = 0x9123_683e;
const TMPFS_MAGIC: u64 = 0x0102_1994;
const HEX: &[u8; 16] = b"0123456789abcdef";
use crate::{
    error::HostError,
    journal::{
        StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver,
        StorageProvisioningPlan, StorageProvisioningRole, StorageRole,
    },
    launch::bootstrap::{BootstrapContainerRole, ObservedContainerCgroup, ObservedStorageRole},
};
use sha2::{Digest, Sha256};
use velnor_resource_probe::{
    InodePolicy,
    worker_binding::{OwnedCgroupObservation, SelfStorageObservation},
};

pub(super) fn observed_roles(
    plan: &StorageProvisioningPlan,
    attachments: &[MeasuredAttachment],
) -> Result<Vec<ObservedStorageRole>, HostError> {
    plan.roles()
        .iter()
        .map(|planned| observed_role(planned, attachments))
        .collect()
}

fn observed_role(
    planned: &StorageProvisioningRole,
    attachments: &[MeasuredAttachment],
) -> Result<ObservedStorageRole, HostError> {
    let contract = planned.role().contract().ok_or(HostError::Identity)?;
    let matching = attachments
        .iter()
        .filter(|attachment| {
            attachment.role == planned.role()
                && (planned.role() != StorageRole::EngineMetadata
                    || attachment.binding.observer() == StorageObserver::Probe)
        })
        .collect::<Vec<_>>();
    let first = matching.first().ok_or(HostError::Identity)?;
    if matching
        .iter()
        .any(|attachment| !same_filesystem(first, attachment))
    {
        return Err(HostError::Identity);
    }
    let measurement = first;
    let filesystem_kind = filesystem_kind(
        measurement.capacity_fs_type_magic,
        &measurement.filesystem_type_name,
    )?;
    let inode_policy = inode_policy(measurement.inode_policy);
    let digest = measurement.capacity_digest.clone();
    if planned.medium() != contract.medium
        || planned.filesystem_kind() != filesystem_kind
        || planned.inode_policy() != inode_policy
        || planned
            .planned_filesystem_instance_digest()
            .is_some_and(|expected| expected != digest)
    {
        return Err(HostError::Identity);
    }
    let (owned_usage_bytes, owned_usage_inodes) = usage_credit(
        planned.medium(),
        measurement.total_bytes,
        measurement.free_bytes,
        measurement.inode_total,
        measurement.inode_free,
    )?;
    Ok(ObservedStorageRole {
        role: planned.role(),
        pool_id: planned.pool_id().to_owned(),
        filesystem_instance_digest: digest,
        medium: planned.medium(),
        filesystem_kind,
        total_bytes: measurement.total_bytes,
        free_bytes: measurement.free_bytes,
        available_bytes: measurement.available_bytes,
        inode_total: Some(measurement.inode_total),
        inode_free: Some(measurement.inode_free),
        inode_available: Some(measurement.inode_available),
        inode_policy,
        charge_bytes: planned.charge_bytes(),
        charge_inodes: planned.charge_inodes(),
        retained_memory_bytes: planned.retained_memory_bytes(),
        owned_usage_bytes,
        owned_usage_inodes,
    })
}

fn same_filesystem(left: &MeasuredAttachment, right: &MeasuredAttachment) -> bool {
    left.capacity_digest == right.capacity_digest
        && left.binding.device_major == right.binding.device_major
        && left.binding.device_minor == right.binding.device_minor
        && left.binding.statx_device_major == right.binding.statx_device_major
        && left.binding.statx_device_minor == right.binding.statx_device_minor
        && left.binding.fs_type_magic == right.binding.fs_type_magic
        && left.binding.filesystem_id == right.binding.filesystem_id
        && left.total_bytes == right.total_bytes
        && left.free_bytes == right.free_bytes
        && left.available_bytes == right.available_bytes
        && left.inode_total == right.inode_total
        && left.inode_free == right.inode_free
        && left.inode_available == right.inode_available
        && left.inode_policy == right.inode_policy
        && left.filesystem_type_name == right.filesystem_type_name
}

fn filesystem_kind(magic: u64, name: &str) -> Result<StorageFilesystemKind, HostError> {
    match (magic, name) {
        (BTRFS_MAGIC, "btrfs") => Ok(StorageFilesystemKind::Btrfs),
        (TMPFS_MAGIC, "tmpfs") => Ok(StorageFilesystemKind::Tmpfs),
        (BTRFS_MAGIC | TMPFS_MAGIC | 0, _) | (_, "btrfs" | "tmpfs" | "") => {
            Err(HostError::Identity)
        }
        _ => Ok(StorageFilesystemKind::Other),
    }
}

fn inode_policy(value: InodePolicy) -> StorageInodePolicy {
    match value {
        InodePolicy::Finite => StorageInodePolicy::Finite,
        InodePolicy::DynamicBtrfs => StorageInodePolicy::DynamicBtrfs,
        InodePolicy::Unknown => StorageInodePolicy::Unknown,
    }
}

pub(super) fn usage_credit(
    medium: StorageMedium,
    total_bytes: u64,
    free_bytes: u64,
    inode_total: u64,
    inode_free: u64,
) -> Result<(u64, u64), HostError> {
    if total_bytes == 0 || free_bytes > total_bytes || inode_free > inode_total {
        return Err(HostError::Identity);
    }
    match medium {
        StorageMedium::Tmpfs => Ok((
            total_bytes
                .checked_sub(free_bytes)
                .ok_or(HostError::Identity)?,
            inode_total
                .checked_sub(inode_free)
                .ok_or(HostError::Identity)?,
        )),
        StorageMedium::Disk => Ok((0, 0)),
    }
}

pub(super) fn observed_cgroup(
    observation: &SelfStorageObservation,
    role: BootstrapContainerRole,
    container_id: &str,
) -> Result<ObservedContainerCgroup, HostError> {
    let limits = &observation.self_leaf_limits;
    if observation.container_id != container_id
        || limits.cgroup_path.is_empty()
        || limits.memory_max_bytes.is_none_or(|value| value == 0)
        || limits.cpu_quota_micros.is_none_or(|value| value == 0)
        || limits.cpu_period_micros == 0
        || limits.pids_max.is_none_or(|value| value == 0)
        || limits.pids_current > limits.pids_max.unwrap_or_default()
    {
        return Err(HostError::Identity);
    }
    Ok(ObservedContainerCgroup {
        role,
        container_id: container_id.to_owned(),
        path: limits.cgroup_path.clone(),
        memory_max_bytes: limits.memory_max_bytes,
        memory_current_bytes: limits.memory_current_bytes,
        cpu_quota_micros: limits.cpu_quota_micros,
        cpu_period_micros: limits.cpu_period_micros,
        pids_max: limits.pids_max,
    })
}

pub(super) fn instance_binding_digest(
    plan: &StorageProvisioningPlan,
    runner_id: &str,
    dind_id: &str,
    boot_id: &str,
    runner: &VerifiedWorkerStorageObservation,
    dind: &VerifiedWorkerStorageObservation,
    owned: &OwnedCgroupObservation,
) -> Result<String, HostError> {
    let runner = serde_json::to_vec(runner.observation()).map_err(|_| HostError::Frame)?;
    let dind = serde_json::to_vec(dind.observation()).map_err(|_| HostError::Frame)?;
    let owned = serde_json::to_vec(owned).map_err(|_| HostError::Frame)?;
    let mut hasher = Sha256::new();
    hasher.update(b"velnor-storage-binding-v1\0");
    let reservation_id = plan.reservation_id().to_be_bytes();
    let operation_id = plan.operation_id().to_be_bytes();
    for field in [
        reservation_id.as_slice(),
        operation_id.as_slice(),
        plan.engine_id().as_bytes(),
        plan.docker_root_digest().as_bytes(),
        plan.sample_generation_id().as_bytes(),
        plan.profile_generation_id().as_bytes(),
        plan.profile_digest().as_bytes(),
        runner_id.as_bytes(),
        dind_id.as_bytes(),
        boot_id.as_bytes(),
        runner.as_slice(),
        dind.as_slice(),
        owned.as_slice(),
    ] {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field);
    }
    let mut digest = String::with_capacity(64);
    for byte in hasher.finalize() {
        digest.push(char::from(HEX[usize::from(byte >> 4)]));
        digest.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(digest)
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn lowercase_hex(bytes: impl IntoIterator<Item = u8>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::new();
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

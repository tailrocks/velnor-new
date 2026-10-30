use crate::error::HostError;
use crate::journal::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
};

use crate::launch::bootstrap::{BootstrapContainerRole, VerifiedStorageResourceBinding};

const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;

pub(super) fn validate_storage_resource_binding(
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    if binding.reservation_id <= 0
        || binding.operation_id <= 0
        || !valid_engine_id(&binding.engine_id)
        || !valid_digest(&binding.docker_root_digest)
        || !valid_generation(&binding.sample_generation_id)
        || !valid_generation(&binding.profile_generation_id)
        || !valid_digest(&binding.profile_digest)
        || !valid_digest(&binding.instance_binding_digest)
        || !valid_container_id(&binding.runner_container_id)
        || !valid_container_id(&binding.dind_container_id)
        || binding.runner_container_id == binding.dind_container_id
        || !valid_uuid(&binding.guest_boot_id)
    {
        return Err(HostError::Journal);
    }
    validate_roles(&binding.roles)?;
    validate_attachments(
        &binding.roles,
        &binding.attachments,
        &binding.runner_container_id,
        &binding.dind_container_id,
    )?;
    validate_cgroups(
        &binding.cgroups,
        &binding.runner_container_id,
        &binding.dind_container_id,
    )
}

fn validate_roles(roles: &[super::ObservedStorageRole]) -> Result<(), HostError> {
    const PHYSICAL: [StorageRole; 11] = [
        StorageRole::RunnerRoot,
        StorageRole::RunnerHome,
        StorageRole::RunnerWork,
        StorageRole::RunnerTmp,
        StorageRole::RunnerShm,
        StorageRole::SharedRun,
        StorageRole::DindRoot,
        StorageRole::DindData,
        StorageRole::DindTmp,
        StorageRole::DindShm,
        StorageRole::EngineMetadata,
    ];
    if roles.len() != PHYSICAL.len()
        || PHYSICAL.iter().any(|role| {
            roles
                .iter()
                .filter(|observed| observed.role == *role)
                .count()
                != 1
        })
    {
        return Err(HostError::Journal);
    }
    for observed in roles {
        validate_role(observed)?;
    }
    Ok(())
}

fn validate_role(role: &super::ObservedStorageRole) -> Result<(), HostError> {
    let contract = role.role.contract().ok_or(HostError::Journal)?;
    let (total, free, available) = role.byte_counts();
    let (inode_total, inode_free, inode_available) = role.inode_counts();
    let (charge_bytes, charge_inodes, retained_memory_bytes) = role.charges();
    let (owned_usage_bytes, owned_usage_inodes) = role.owned_usage();
    let bytes_cover = available
        .checked_add(owned_usage_bytes)
        .is_some_and(|capacity| capacity >= charge_bytes);
    let inode_shape = match role.inode_policy() {
        StorageInodePolicy::Finite => match (inode_total, inode_free, inode_available) {
            (Some(total), Some(free), Some(available)) => {
                total > 0
                    && free <= total
                    && available <= free
                    && charge_inodes > 0
                    && available
                        .checked_add(owned_usage_inodes)
                        .is_some_and(|capacity| capacity >= charge_inodes)
            }
            _ => false,
        },
        StorageInodePolicy::DynamicBtrfs => {
            role.medium() == StorageMedium::Disk
                && role.filesystem_kind() == StorageFilesystemKind::Btrfs
                && charge_inodes > 0
                && owned_usage_inodes == 0
                && inode_total == Some(0)
                && inode_free == Some(0)
                && inode_available == Some(0)
        }
        StorageInodePolicy::Unknown => false,
    };
    let medium_shape = match role.medium() {
        StorageMedium::Tmpfs => {
            role.filesystem_kind() == StorageFilesystemKind::Tmpfs
                && role.inode_policy() == StorageInodePolicy::Finite
                && retained_memory_bytes == charge_bytes
        }
        StorageMedium::Disk => {
            role.filesystem_kind() != StorageFilesystemKind::Tmpfs && retained_memory_bytes == 0
        }
    };
    if !valid_digest(role.pool_id())
        || !valid_digest(role.filesystem_instance_digest())
        || role.medium() != contract.medium
        || total == 0
        || free > total
        || available > free
        || charge_bytes == 0
        || owned_usage_bytes > charge_bytes
        || !bytes_cover
        || !inode_shape
        || !medium_shape
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn validate_attachments(
    roles: &[super::ObservedStorageRole],
    attachments: &[super::ObservedStorageAttachment],
    runner_container_id: &str,
    dind_container_id: &str,
) -> Result<(), HostError> {
    const PAIRS: [(StorageRole, StorageObserver); 14] = [
        (StorageRole::RunnerRoot, StorageObserver::Runner),
        (StorageRole::RunnerHome, StorageObserver::Runner),
        (StorageRole::RunnerHome, StorageObserver::Dind),
        (StorageRole::RunnerWork, StorageObserver::Runner),
        (StorageRole::RunnerWork, StorageObserver::Dind),
        (StorageRole::RunnerTmp, StorageObserver::Runner),
        (StorageRole::RunnerShm, StorageObserver::Runner),
        (StorageRole::SharedRun, StorageObserver::Runner),
        (StorageRole::SharedRun, StorageObserver::Dind),
        (StorageRole::DindRoot, StorageObserver::Dind),
        (StorageRole::DindData, StorageObserver::Dind),
        (StorageRole::DindTmp, StorageObserver::Dind),
        (StorageRole::DindShm, StorageObserver::Dind),
        (StorageRole::EngineMetadata, StorageObserver::Probe),
    ];
    if attachments.len() != PAIRS.len()
        || PAIRS.iter().any(|(role, observer)| {
            attachments
                .iter()
                .filter(|item| item.role == *role && item.observer == *observer)
                .count()
                != 1
        })
    {
        return Err(HostError::Journal);
    }
    for item in attachments {
        let contract = item.role.contract().ok_or(HostError::Journal)?;
        let expected_container = match item.observer {
            StorageObserver::Runner => Some(runner_container_id),
            StorageObserver::Dind => Some(dind_container_id),
            StorageObserver::Probe => None,
        };
        let observed_role = roles
            .iter()
            .find(|role| role.role == item.role)
            .ok_or(HostError::Journal)?;
        let device_matches = item.fs_type_magic == BTRFS_SUPER_MAGIC
            || (item.device_major, item.device_minor)
                == (item.statx_device_major, item.statx_device_minor);
        if item.observer != contract.observer
            && !matches!(
                (item.role, item.observer),
                (
                    StorageRole::RunnerHome | StorageRole::RunnerWork | StorageRole::SharedRun,
                    StorageObserver::Dind
                )
            )
        {
            return Err(HostError::Journal);
        }
        if item.target != contract.target
            || item.source_target.as_deref() != contract.source_target
            || item.read_only != contract.read_only
            || item.container_id.as_deref() != expected_container
            || item.mount_id == 0
            || item.fs_type_magic == 0
            || item.filesystem_id == [0, 0]
            || !device_matches
            || !valid_digest(&item.filesystem_instance_digest)
            || item.filesystem_instance_digest != observed_role.filesystem_instance_digest
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

fn validate_cgroups(
    cgroups: &[super::ObservedContainerCgroup],
    runner_container_id: &str,
    dind_container_id: &str,
) -> Result<(), HostError> {
    if cgroups.len() != 2 {
        return Err(HostError::Journal);
    }
    for (role, container_id) in [
        (BootstrapContainerRole::Runner, runner_container_id),
        (BootstrapContainerRole::Dind, dind_container_id),
    ] {
        let matching = cgroups
            .iter()
            .filter(|cgroup| cgroup.role == role && cgroup.container_id == container_id)
            .count();
        if matching != 1 {
            return Err(HostError::Journal);
        }
    }
    for cgroup in cgroups {
        let memory_fits = cgroup
            .memory_max_bytes
            .is_some_and(|limit| limit > 0 && limit >= cgroup.memory_current_bytes);
        let cpu_is_bounded = cgroup
            .cpu_quota_micros
            .is_some_and(|quota| quota > 0 && cgroup.cpu_period_micros > 0);
        let pids_are_bounded = cgroup.pids_max.is_some_and(|limit| limit > 0);
        if !valid_container_id(&cgroup.container_id)
            || !valid_bounded(&cgroup.path, 1024)
            || !memory_fits
            || !cpu_is_bounded
            || !pids_are_bounded
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    valid_lower_hex(value, 64)
}

fn valid_generation(value: &str) -> bool {
    valid_lower_hex(value, 32)
}

fn valid_container_id(value: &str) -> bool {
    valid_lower_hex(value, 64)
}

fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_engine_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn valid_bounded(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.bytes().all(|byte| !byte.is_ascii_control())
}

#[cfg(test)]
mod tests {
    use super::valid_engine_id;

    #[test]
    fn engine_id_requires_the_daemon_uuid_shape() {
        assert!(valid_engine_id("bc9058a0-c807-412b-a088-6c1d96ddd462"));
        assert!(!valid_engine_id("test-engine"));
    }
}

mod inventory;

use crate::error::HostError;
use crate::journal::{StorageObserver, StorageRole};

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
        || binding.cleanup_observer_binding.expected_guest_boot_id != binding.guest_boot_id
        || velnor_resource_probe::cleanup_observer_wire::validate_binding(
            &binding.cleanup_observer_binding,
        )
        .is_err()
    {
        return Err(HostError::Journal);
    }
    inventory::validate_roles(&binding.roles)?;
    inventory::validate_attachments(
        &binding.roles,
        &binding.attachments,
        &binding.runner_container_id,
        &binding.dind_container_id,
    )?;
    validate_cgroups(
        &binding.cgroups,
        &binding.runner_container_id,
        &binding.dind_container_id,
    )?;
    validate_cleanup_binding_matches_attachments(binding)
}

fn validate_cleanup_binding_matches_attachments(
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    let cleanup = &binding.cleanup_observer_binding;
    for volume in &cleanup.volume_mountpoints {
        let role = StorageRole::parse(&volume.role).map_err(|_| HostError::Journal)?;
        if role == StorageRole::EngineMetadata {
            let root = metadata_subpath_attachment(binding)?;
            if volume.mount_device != [root.device().0, root.device().1]
                || volume.statx_device != [root.statx_device().0, root.statx_device().1]
            {
                return Err(HostError::Journal);
            }
            continue;
        }
        let root = binding.cleanup_volume_root_attachment(role)?;
        if volume.mount_device != [root.device().0, root.device().1]
            || volume.statx_device != [root.statx_device().0, root.statx_device().1]
            || volume.inode != root.statx_inode()
        {
            return Err(HostError::Journal);
        }
    }
    let docker_root = binding.cleanup_volume_root_attachment(StorageRole::EngineMetadata)?;
    if cleanup.docker_root.mount_id != docker_root.mount_id()
        || cleanup.docker_root.statx_device
            != [docker_root.statx_device().0, docker_root.statx_device().1]
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn metadata_subpath_attachment(
    binding: &VerifiedStorageResourceBinding,
) -> Result<&super::ObservedStorageAttachment, HostError> {
    let mut matches = binding.attachments.iter().filter(|item| {
        item.role() == StorageRole::EngineMetadata
            && item.observer() == StorageObserver::Runner
            && item.container_id() == Some(binding.runner_container_id.as_str())
            && item.target() == "/opt/velnor/job-facts"
            && item.volume_subpath() == Some("job-facts")
            && item.read_only()
    });
    let row = matches.next().ok_or(HostError::Journal)?;
    if matches.next().is_some() {
        return Err(HostError::Journal);
    }
    Ok(row)
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

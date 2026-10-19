use crate::{
    ProbeError,
    cleanup_observer_wire::{CleanupObserverRequest, ProcessIdentity},
};

use super::{MAX_CLEANUP_VOLUMES, OWNED_TMPFS_DEVICE_COUNT};

pub(super) fn validate(request: &CleanupObserverRequest) -> Result<(), ProbeError> {
    if request.schema_version != 1
        || !lower_hex(&request.operation_id, 32)
        || !lower_hex(&request.binding_sha256, 64)
        || !valid_boot_id(&request.binding.expected_guest_boot_id)
    {
        return Err(ProbeError::Invalid("cleanup_request_identity"));
    }
    let binding = &request.binding;
    validate_process(&binding.daemon)?;
    validate_process(&binding.runner)?;
    validate_process(&binding.dind)?;
    if binding.docker_root.target != "/velnor/docker-root"
        || binding.docker_root.mount_id == 0
        || binding.docker_root.mount_id != binding.docker_root.statx_mount_id
        || binding.docker_root.mount_device != binding.docker_root.statx_device
        || binding.docker_root.filesystem_id == 0
        || !binding.docker_root.read_only
    {
        return Err(ProbeError::Invalid("cleanup_docker_root_identity"));
    }
    if binding.daemon.pid == binding.runner.pid
        || binding.daemon.pid == binding.dind.pid
        || binding.runner.pid == binding.dind.pid
        || binding.runner.namespace_inode == binding.dind.namespace_inode
        || binding.cgroup_paths[0] == binding.cgroup_paths[1]
        || binding.volume_mountpoints.len() > MAX_CLEANUP_VOLUMES
        || !matches!(binding.volume_mountpoints.len(), 4 | 5)
    {
        return Err(ProbeError::Invalid("cleanup_binding_shape"));
    }
    let expected_roles: &[&str] = if binding.volume_mountpoints.len() == 4 {
        &["shared-run", "runner-home", "runner-work", "dind-data"]
    } else {
        &[
            "shared-run",
            "runner-home",
            "runner-work",
            "dind-data",
            "engine-metadata",
        ]
    };
    for (volume, expected_role) in binding.volume_mountpoints.iter().zip(expected_roles) {
        if volume.role != *expected_role {
            return Err(ProbeError::Invalid("cleanup_volume_role"));
        }
        validate_absolute_path(&volume.mountpoint, "cleanup_volume_mountpoint")?;
        if volume.mount_device == [0, 0] || volume.statx_device == [0, 0] || volume.inode == 0 {
            return Err(ProbeError::Invalid("cleanup_volume_identity"));
        }
    }
    if binding.owned_tmpfs_devices.contains(&[0, 0]) {
        return Err(ProbeError::Invalid("cleanup_tmpfs_device"));
    }
    let mut devices = binding.owned_tmpfs_devices.to_vec();
    devices.sort_unstable();
    devices.dedup();
    if devices.len() != OWNED_TMPFS_DEVICE_COUNT {
        return Err(ProbeError::Invalid("cleanup_tmpfs_duplicate"));
    }
    for path in &binding.cgroup_paths {
        validate_absolute_path(path, "cleanup_cgroup_path")?;
    }
    Ok(())
}

fn validate_process(process: &ProcessIdentity) -> Result<(), ProbeError> {
    if process.pid == 0
        || process.start_ticks == 0
        || process.namespace_inode == 0
        || process.namespace_device == [0, 0]
    {
        return Err(ProbeError::Invalid("cleanup_process_identity"));
    }
    Ok(())
}

fn valid_boot_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_absolute_path(value: &str, error: &'static str) -> Result<(), ProbeError> {
    if value.len() > 4096
        || value == "/"
        || !value.starts_with('/')
        || value.contains("//")
        || value.split('/').skip(1).any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.bytes().any(|byte| {
                    !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
                })
        })
    {
        return Err(ProbeError::Invalid(error));
    }
    Ok(())
}

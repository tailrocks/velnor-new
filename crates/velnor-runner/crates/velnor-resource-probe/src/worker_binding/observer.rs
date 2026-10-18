mod cgroup;
mod process;

use crate::{
    ProbeError,
    filesystem::{filesystem_name, observe_docker_root, parse_boot_id},
    mountinfo::cgroup2_root,
    worker_binding::{
        AttachmentRole, OwnedCgroupObservation, OwnedCgroupRequest, StorageAttachmentObservation,
        WorkerRole,
    },
};

const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
const MOUNTINFO_LIMIT: usize = 65_536;

pub(super) fn observe(request: OwnedCgroupRequest) -> Result<OwnedCgroupObservation, ProbeError> {
    request.validate()?;
    let boot_id = parse_boot_id(&read_bounded(BOOT_ID_PATH, 128, "boot_id")?)?;
    let mountinfo = read_bounded(MOUNTINFO_PATH, MOUNTINFO_LIMIT, "mountinfo")?;
    let cgroup_mount = cgroup2_root(&mountinfo)?;
    let engine_metadata = observe_engine_metadata(&mountinfo)?;
    let containers = vec![
        process::observe_container(
            WorkerRole::Runner,
            &request.runner_container_id,
            request.runner_init_pid,
            request.runner_init_start_ticks,
            request.runner_helper_pid,
            request.runner_helper_start_ticks,
        )?,
        process::observe_container(
            WorkerRole::Dind,
            &request.dind_container_id,
            request.dind_init_pid,
            request.dind_init_start_ticks,
            request.dind_helper_pid,
            request.dind_helper_start_ticks,
        )?,
    ];
    if containers[0].init.cgroup_path == containers[1].init.cgroup_path {
        return Err(ProbeError::Invalid("worker_cgroup_not_separate"));
    }
    let mountinfo_after = read_bounded(MOUNTINFO_PATH, MOUNTINFO_LIMIT, "mountinfo")?;
    let engine_metadata_after = observe_engine_metadata(&mountinfo_after)?;
    if cgroup2_root(&mountinfo_after)? != cgroup_mount
        || parse_boot_id(&read_bounded(BOOT_ID_PATH, 128, "boot_id")?)? != boot_id
        || !same_filesystem_identity(&engine_metadata, &engine_metadata_after)
    {
        return Err(ProbeError::Invalid("observer_identity_changed"));
    }
    Ok(OwnedCgroupObservation {
        wire_mode: "owned_container_cgroup".to_owned(),
        schema_version: 1,
        operation_id: request.operation_id,
        guest_boot_id: boot_id,
        engine_metadata,
        containers,
    })
}

fn same_filesystem_identity(
    before: &StorageAttachmentObservation,
    after: &StorageAttachmentObservation,
) -> bool {
    before.role == after.role
        && before.target == after.target
        && before.mount_id == after.mount_id
        && before.statx_mount_id == after.statx_mount_id
        && before.mount_root == after.mount_root
        && before.mount_device_major == after.mount_device_major
        && before.mount_device_minor == after.mount_device_minor
        && before.statx_device_major == after.statx_device_major
        && before.statx_device_minor == after.statx_device_minor
        && before.filesystem_type_magic == after.filesystem_type_magic
        && before.filesystem_type_name == after.filesystem_type_name
        && before.filesystem_id == after.filesystem_id
}

fn observe_engine_metadata(mountinfo: &[u8]) -> Result<StorageAttachmentObservation, ProbeError> {
    let filesystem = observe_docker_root(mountinfo)?;
    let filesystem_type_name = filesystem_name(&filesystem.mount, filesystem.fs_type_magic)?;
    Ok(StorageAttachmentObservation {
        role: AttachmentRole::EngineMetadata,
        target: filesystem.mount_target.to_owned(),
        read_only: filesystem.mount.read_only,
        executable_mount: filesystem.mount.executable,
        mount_id: filesystem.mount.mount_id,
        statx_mount_id: filesystem.statx_mount_id,
        mount_root: filesystem.mount.root,
        mount_device_major: filesystem.mount.device_major,
        mount_device_minor: filesystem.mount.device_minor,
        statx_device_major: filesystem.statx_device_major,
        statx_device_minor: filesystem.statx_device_minor,
        filesystem_type_magic: filesystem.fs_type_magic,
        filesystem_type_name,
        filesystem_id: filesystem.filesystem_id,
        block_size: filesystem.block_size,
        total_bytes: filesystem.total_bytes,
        free_bytes: filesystem.free_bytes,
        available_bytes: filesystem.available_bytes,
        inode_total: filesystem.total_inodes,
        inode_free: filesystem.free_inodes,
        inode_available: filesystem.available_inodes,
        inode_policy: filesystem.inode_policy,
    })
}

fn read_bounded(path: &str, limit: usize, name: &'static str) -> Result<Vec<u8>, ProbeError> {
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take(u64::try_from(limit + 1).map_err(|_| ProbeError::Overflow("input_limit"))?)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}

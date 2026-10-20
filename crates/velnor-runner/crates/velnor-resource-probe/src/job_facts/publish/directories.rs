use super::{BTRFS_MAGIC, DIRECTORY_MODE, ROOT_GID, ROOT_UID, RUNTIME_DIRECTORY_TARGET};
use crate::{ProbeError, execution_channel_wire::ExecutionChannelDirectory, mountinfo::MountEntry};
use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, StatxFlags, fstat, fstatfs, fstatvfs, open, openat, statx,
};
use std::fs::File;

pub(super) fn open_directory(name: &str) -> Result<File, ProbeError> {
    let mut current = File::from(
        open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| ProbeError::Invalid("job_facts_root_open"))?,
    );
    for component in ["opt", "velnor", name] {
        current = File::from(
            openat(
                &current,
                component,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| ProbeError::Invalid("job_facts_directory_open"))?,
        );
    }
    Ok(current)
}

pub(super) fn observe_directory(
    directory: &File,
    mount: &MountEntry,
    source: &str,
    target: &str,
) -> Result<ExecutionChannelDirectory, ProbeError> {
    let stat = fstat(directory).map_err(|_| ProbeError::Invalid("job_facts_directory_stat"))?;
    let statx_value = statx(
        directory,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| ProbeError::Invalid("job_facts_directory_statx"))?;
    let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
    if statx_value.stx_mask & required.bits() != required.bits()
        || FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != ROOT_UID
        || stat.st_gid != ROOT_GID
        || stat.st_mode & 0o7777 != DIRECTORY_MODE
        || stat.st_ino == 0
        || statx_value.stx_ino != stat.st_ino
        || statx_value.stx_uid != ROOT_UID
        || statx_value.stx_gid != ROOT_GID
        || statx_value.stx_mnt_id != mount.mount_id
        || mount.read_only
    {
        return Err(ProbeError::Invalid("job_facts_directory_identity"));
    }
    let filesystem =
        fstatfs(directory).map_err(|_| ProbeError::Invalid("job_facts_directory_statfs"))?;
    let capacity =
        fstatvfs(directory).map_err(|_| ProbeError::Invalid("job_facts_directory_statvfs"))?;
    let magic = u64::try_from(filesystem.f_type)
        .map_err(|_| ProbeError::Invalid("job_facts_filesystem_type"))?;
    Ok(ExecutionChannelDirectory {
        target: target.to_owned(),
        inode: stat.st_ino,
        owner_uid: ROOT_UID,
        owner_gid: ROOT_GID,
        mode: stat.st_mode & 0o7777,
        mount_id: mount.mount_id,
        statx_mount_id: statx_value.stx_mnt_id,
        mount_root: mount.root.clone(),
        mount_source: source.to_owned(),
        mount_device_major: mount.device_major,
        mount_device_minor: mount.device_minor,
        statx_device_major: statx_value.stx_dev_major,
        statx_device_minor: statx_value.stx_dev_minor,
        filesystem_type_magic: magic,
        filesystem_type_name: mount.filesystem_type.clone(),
        filesystem_id: capacity.f_fsid,
        read_only: false,
        executable_mount: mount.executable,
    })
}

pub(super) fn compare_runtime_directory(
    expected: &crate::job_facts_wire::JobFactsRuntimeIdentityDirectory,
    observed: &ExecutionChannelDirectory,
) -> Result<(), ProbeError> {
    if observed.target != RUNTIME_DIRECTORY_TARGET
        || observed.inode != expected.inode
        || observed.owner_uid != expected.owner_uid
        || observed.owner_gid != expected.owner_gid
        || observed.mode != expected.mode
        || observed.statx_device_major != expected.device_major
        || observed.statx_device_minor != expected.device_minor
        || observed.read_only
    {
        return Err(ProbeError::Invalid("job_facts_runtime_binding"));
    }
    Ok(())
}

pub(super) fn compare_runner_directory(
    expected: &ExecutionChannelDirectory,
    observed: &ExecutionChannelDirectory,
) -> Result<(), ProbeError> {
    let btrfs = observed.filesystem_type_magic == BTRFS_MAGIC;
    if observed.target != expected.target
        || observed.inode != expected.inode
        || observed.owner_uid != expected.owner_uid
        || observed.owner_gid != expected.owner_gid
        || observed.mode != expected.mode
        || observed.filesystem_type_magic != expected.filesystem_type_magic
        || observed.filesystem_type_name != expected.filesystem_type_name
        || observed.filesystem_id != expected.filesystem_id
        || !crate::job_facts_wire::mount_root_contains_subpath(
            &observed.mount_root,
            &expected.mount_root,
            "job-facts",
        )
        || observed.mount_source != expected.mount_source
        || observed.mount_device_major != expected.mount_device_major
        || observed.mount_device_minor != expected.mount_device_minor
        || (!btrfs
            && (observed.statx_device_major != expected.statx_device_major
                || observed.statx_device_minor != expected.statx_device_minor))
        || observed.read_only
    {
        return Err(ProbeError::Invalid("job_facts_runner_binding"));
    }
    Ok(())
}

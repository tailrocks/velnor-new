use crate::{
    ProbeError,
    worker_binding::{StorageAttachmentObservation, WorkerRole},
};

#[cfg(target_os = "linux")]
use crate::{
    filesystem::{filesystem_name, inode_policy},
    mountinfo::{attachment_mount, attachment_mount_source},
    worker_binding::{AttachmentSpec, DIND_ATTACHMENTS, RUNNER_ATTACHMENTS},
};

#[cfg(target_os = "linux")]
use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, StatxFlags, fstat, fstatfs, fstatvfs, open, openat, statx,
};

#[cfg(target_os = "linux")]
use std::os::fd::OwnedFd;

#[cfg(target_os = "linux")]
pub(super) fn observe_attachments(
    role: WorkerRole,
    mountinfo: &[u8],
) -> Result<Vec<StorageAttachmentObservation>, ProbeError> {
    let specs = match role {
        WorkerRole::Runner => &RUNNER_ATTACHMENTS[..],
        WorkerRole::Dind => &DIND_ATTACHMENTS[..],
    };
    specs
        .iter()
        .map(|spec| observe_attachment(*spec, mountinfo))
        .collect()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn observe_attachments(
    _role: WorkerRole,
    _mountinfo: &[u8],
) -> Result<Vec<StorageAttachmentObservation>, ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}

#[cfg(target_os = "linux")]
fn observe_attachment(
    spec: AttachmentSpec,
    mountinfo: &[u8],
) -> Result<StorageAttachmentObservation, ProbeError> {
    let descriptor = open_target(spec.target)?;
    let status =
        fstat(&descriptor).map_err(|error| ProbeError::Read("attachment_stat", error.into()))?;
    if FileType::from_raw_mode(status.st_mode) != FileType::Directory {
        return Err(ProbeError::Invalid("attachment_not_directory"));
    }
    let statx_value = statx(
        &descriptor,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|error| ProbeError::Read("attachment_statx", error.into()))?;
    let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
    if statx_value.stx_mask & required.bits() != required.bits() || statx_value.stx_ino == 0 {
        return Err(ProbeError::Invalid("attachment_statx_fields"));
    }
    let filesystem = fstatfs(&descriptor)
        .map_err(|error| ProbeError::Read("attachment_statfs", error.into()))?;
    let capacity = fstatvfs(&descriptor)
        .map_err(|error| ProbeError::Read("attachment_statvfs", error.into()))?;
    let mount = attachment_mount(mountinfo, spec.target)?;
    let mount_source = attachment_mount_source(mountinfo, spec.target)?;
    let magic =
        u64::try_from(filesystem.f_type).map_err(|_| ProbeError::Invalid("attachment_fs_type"))?;
    if mount.mount_id != statx_value.stx_mnt_id
        || (magic != crate::filesystem::BTRFS_SUPER_MAGIC
            && (mount.device_major != statx_value.stx_dev_major
                || mount.device_minor != statx_value.stx_dev_minor))
        || (magic == crate::filesystem::BTRFS_SUPER_MAGIC) != (mount.filesystem_type == "btrfs")
    {
        return Err(ProbeError::Invalid("attachment_mount_identity"));
    }
    let block_size = capacity.f_frsize;
    if block_size == 0 {
        return Err(ProbeError::Invalid("attachment_block_size"));
    }
    let total_bytes = checked_bytes(capacity.f_blocks, block_size, "attachment_total")?;
    let free_bytes = checked_bytes(capacity.f_bfree, block_size, "attachment_free")?;
    let available_bytes = checked_bytes(capacity.f_bavail, block_size, "attachment_available")?;
    if free_bytes > total_bytes || available_bytes > free_bytes {
        return Err(ProbeError::Invalid("attachment_capacity"));
    }
    let inode_total = capacity.f_files;
    let inode_free = capacity.f_ffree;
    let inode_available = capacity.f_favail;
    let inode_policy = inode_policy(magic, inode_total, inode_free, inode_available)?;
    let filesystem_type_name = filesystem_name(&mount, magic)?;
    Ok(StorageAttachmentObservation {
        role: spec.role,
        target: spec.target.to_owned(),
        read_only: mount.read_only,
        executable_mount: mount.executable,
        mount_id: mount.mount_id,
        statx_mount_id: statx_value.stx_mnt_id,
        statx_inode: statx_value.stx_ino,
        mount_root: mount.root,
        mount_source,
        mount_device_major: mount.device_major,
        mount_device_minor: mount.device_minor,
        statx_device_major: statx_value.stx_dev_major,
        statx_device_minor: statx_value.stx_dev_minor,
        filesystem_type_magic: magic,
        filesystem_type_name,
        filesystem_id: capacity.f_fsid,
        block_size,
        total_bytes,
        free_bytes,
        available_bytes,
        inode_total,
        inode_free,
        inode_available,
        inode_policy,
    })
}

#[cfg(target_os = "linux")]
fn open_target(path: &str) -> Result<OwnedFd, ProbeError> {
    if !path.starts_with('/') || path.contains("//") || path.contains("..") {
        return Err(ProbeError::Invalid("attachment_path"));
    }
    let root = open(
        "/",
        OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| ProbeError::Read("attachment_root", error.into()))?;
    if path == "/" {
        return Ok(root);
    }
    let mut descriptor = root;
    for component in path.split('/').skip(1) {
        if component.is_empty() || component == "." || component == ".." {
            return Err(ProbeError::Invalid("attachment_path"));
        }
        descriptor = openat(
            &descriptor,
            component,
            OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| ProbeError::Read("attachment_open", error.into()))?;
    }
    Ok(descriptor)
}

#[cfg(any(target_os = "linux", test))]
fn checked_bytes(blocks: u64, block_size: u64, error: &'static str) -> Result<u64, ProbeError> {
    blocks
        .checked_mul(block_size)
        .ok_or(ProbeError::Overflow(error))
}

#[cfg(test)]
mod tests {
    use super::checked_bytes;
    use crate::ProbeError;

    #[test]
    fn byte_counts_use_checked_multiplication() -> Result<(), ProbeError> {
        assert_eq!(checked_bytes(4, 4096, "bytes")?, 16_384);
        assert!(checked_bytes(u64::MAX, 2, "bytes").is_err());
        Ok(())
    }
}

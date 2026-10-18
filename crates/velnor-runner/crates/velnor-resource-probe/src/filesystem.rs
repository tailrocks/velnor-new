#[cfg(target_os = "linux")]
use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, StatxFlags, fstat, fstatfs, fstatvfs, open, statx,
};

#[cfg(any(target_os = "linux", test))]
use crate::capacity::docker_root_capacity;
use crate::mountinfo::MountEntry;
#[cfg(any(target_os = "linux", test))]
use crate::mountinfo::docker_root_mount;
use crate::{ProbeError, units::digits};

#[cfg(any(target_os = "linux", test))]
pub(crate) const DOCKER_ROOT: &str = "/velnor/docker-root";
pub(crate) const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;
const TMPFS_MAGIC: u64 = 0x0102_1994;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
/// States whether inode counts can bound storage admission.
pub enum InodePolicy {
    /// The filesystem reports finite inode counts.
    Finite,
    /// Btrfs reports zero because it allocates inodes dynamically.
    DynamicBtrfs,
    /// The probe cannot prove a safe inode policy.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilesystemObservation {
    pub(crate) mount: MountEntry,
    pub(crate) mount_target: &'static str,
    pub(crate) statx_device_major: u32,
    pub(crate) statx_device_minor: u32,
    pub(crate) fs_type_magic: u64,
    pub(crate) filesystem_id: u64,
    pub(crate) total_bytes: u64,
    pub(crate) free_bytes: u64,
    pub(crate) available_bytes: u64,
    pub(crate) total_inodes: u64,
    pub(crate) free_inodes: u64,
    pub(crate) available_inodes: u64,
    pub(crate) inode_policy: InodePolicy,
}

/// Read the fixed target through one retained path descriptor.
#[cfg(target_os = "linux")]
pub(crate) fn observe_docker_root(mountinfo: &[u8]) -> Result<FilesystemObservation, ProbeError> {
    let descriptor = open(
        DOCKER_ROOT,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| ProbeError::Read("docker_root", error.into()))?;
    let metadata =
        fstat(&descriptor).map_err(|error| ProbeError::Read("docker_root", error.into()))?;
    if FileType::from_raw_mode(metadata.st_mode) != FileType::Directory {
        return Err(ProbeError::Invalid("docker_root_not_directory"));
    }
    let mount_stat = statx(
        &descriptor,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|error| ProbeError::Read("docker_root_statx", error.into()))?;
    let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
    if mount_stat.stx_mask & required.bits() != required.bits() {
        return Err(ProbeError::Invalid("docker_root_statx_fields"));
    }
    let fs = fstatfs(&descriptor)
        .map_err(|error| ProbeError::Read("docker_root_statfs", error.into()))?;
    let vfs = fstatvfs(&descriptor)
        .map_err(|error| ProbeError::Read("docker_root_statvfs", error.into()))?;
    let mount = docker_root_mount(mountinfo)?;
    let device_major = mount_stat.stx_dev_major;
    let device_minor = mount_stat.stx_dev_minor;
    let fs_type_magic =
        u64::try_from(fs.f_type).map_err(|_| ProbeError::Invalid("docker_root_fs_type"))?;
    validate_mount_identity(
        &mount,
        mount_stat.stx_mnt_id,
        device_major,
        device_minor,
        fs_type_magic,
    )?;
    let (free_bytes, total_bytes, available_bytes) =
        docker_root_capacity(vfs.f_blocks, vfs.f_bfree, vfs.f_bavail, vfs.f_frsize)?;
    let inode_policy = inode_policy(fs_type_magic, vfs.f_files, vfs.f_ffree, vfs.f_favail)?;
    Ok(FilesystemObservation {
        mount,
        mount_target: DOCKER_ROOT,
        statx_device_major: device_major,
        statx_device_minor: device_minor,
        fs_type_magic,
        filesystem_id: vfs.f_fsid,
        total_bytes,
        free_bytes,
        available_bytes,
        total_inodes: vfs.f_files,
        free_inodes: vfs.f_ffree,
        available_inodes: vfs.f_favail,
        inode_policy,
    })
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn validate_mount_identity(
    mount: &MountEntry,
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
    fs_type_magic: u64,
) -> Result<(), ProbeError> {
    let is_btrfs = fs_type_magic == BTRFS_SUPER_MAGIC;
    let btrfs_matches = is_btrfs == (mount.filesystem_type == "btrfs");
    // Btrfs subvolumes can report a distinct st_dev from mountinfo's device.
    let device_matches =
        is_btrfs || (mount.device_major == device_major && mount.device_minor == device_minor);
    if mount.mount_id != mount_id || !device_matches || !mount.read_only || !btrfs_matches {
        return Err(ProbeError::Invalid("docker_root_mount_identity"));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn observe_docker_root(_mountinfo: &[u8]) -> Result<FilesystemObservation, ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn inode_policy(
    fs_type_magic: u64,
    total: u64,
    free: u64,
    available: u64,
) -> Result<InodePolicy, ProbeError> {
    if free > total || available > total {
        return Err(ProbeError::Invalid("docker_root_inode_capacity"));
    }
    if available > free {
        return Err(ProbeError::Invalid("docker_root_inode_capacity"));
    }
    if fs_type_magic == BTRFS_SUPER_MAGIC {
        return Ok(if total == 0 && free == 0 && available == 0 {
            InodePolicy::DynamicBtrfs
        } else {
            InodePolicy::Unknown
        });
    }
    if total > 0 {
        return Ok(InodePolicy::Finite);
    }
    Ok(InodePolicy::Unknown)
}

pub(crate) fn parse_boot_id(input: &[u8]) -> Result<String, ProbeError> {
    let text = std::str::from_utf8(input)
        .map_err(|_| ProbeError::Invalid("boot_id_utf8"))?
        .trim_end_matches('\n');
    if text.len() != 36
        || !text.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        return Err(ProbeError::Invalid("boot_id_format"));
    }
    Ok(text.to_ascii_lowercase())
}

pub(crate) fn filesystem_name(mount: &MountEntry, magic: u64) -> Result<String, ProbeError> {
    match (mount.filesystem_type.as_str(), magic) {
        ("btrfs", BTRFS_SUPER_MAGIC) => Ok("btrfs".to_owned()),
        ("tmpfs", TMPFS_MAGIC) => Ok("tmpfs".to_owned()),
        (name, _) if !name.is_empty() => Ok(name.to_owned()),
        _ => Err(ProbeError::Invalid("docker_root_fs_type_name")),
    }
}

pub(crate) fn parse_u64(value: &str, error: &'static str) -> Result<u64, ProbeError> {
    if !digits(value) {
        return Err(ProbeError::Invalid(error));
    }
    value.parse().map_err(|_| ProbeError::Overflow(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inode_policy_distinguishes_finite_dynamic_and_unknown() -> Result<(), ProbeError> {
        assert_eq!(inode_policy(0x1234, 100, 50, 40)?, InodePolicy::Finite);
        assert_eq!(
            inode_policy(BTRFS_SUPER_MAGIC, 0, 0, 0)?,
            InodePolicy::DynamicBtrfs
        );
        assert_eq!(
            inode_policy(BTRFS_SUPER_MAGIC, 100, 50, 40)?,
            InodePolicy::Unknown
        );
        assert_eq!(inode_policy(0x1234, 0, 0, 0)?, InodePolicy::Unknown);
        assert!(inode_policy(0x1234, 10, 11, 0).is_err());
        assert!(inode_policy(0x1234, 10, 0, 11).is_err());
        assert!(inode_policy(0x1234, 10, 5, 6).is_err());
        Ok(())
    }

    #[test]
    fn mount_identity_checks_filesystem_specific_device_semantics() {
        let mount = MountEntry {
            mount_id: 238,
            device_major: 0,
            device_minor: 38,
            filesystem_type: "btrfs".to_owned(),
            read_only: true,
        };
        // This matches the selected Docker-root observation: mountinfo 0:38, statx 0:41.
        assert!(validate_mount_identity(&mount, 238, 0, 41, BTRFS_SUPER_MAGIC).is_ok());
        assert!(validate_mount_identity(&mount, 237, 0, 41, BTRFS_SUPER_MAGIC).is_err());
        assert!(validate_mount_identity(&mount, 238, 0, 38, 0x1234).is_err());

        let ext4 = MountEntry {
            filesystem_type: "ext4".to_owned(),
            ..mount
        };
        assert!(validate_mount_identity(&ext4, 238, 0, 38, 0xEF53).is_ok());
        assert!(validate_mount_identity(&ext4, 238, 0, 41, 0xEF53).is_err());
    }

    #[test]
    fn boot_identity_is_a_canonical_uuid() -> Result<(), ProbeError> {
        assert_eq!(
            parse_boot_id(b"12345678-1234-1234-1234-123456789ABC\n")?,
            "12345678-1234-1234-1234-123456789abc"
        );
        assert!(parse_boot_id(b"not-a-boot-id\n").is_err());
        assert!(parse_boot_id(b"12345678-1234-1234-1234-123456789abc\nsecond\n").is_err());
        Ok(())
    }
}

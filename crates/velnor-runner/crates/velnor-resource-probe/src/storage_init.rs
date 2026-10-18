//! Fixed, bounded storage initialization contract.

use serde::Deserialize;
#[cfg(target_os = "linux")]
use serde::Serialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub(super) operation_id: String,
}

#[derive(Debug, Serialize)]
#[cfg(target_os = "linux")]
pub(super) struct ResultRecord {
    pub(super) schema: u8,
    pub(super) operation_id: String,
    pub(super) home: DirectoryRecord,
    pub(super) work: DirectoryRecord,
}

#[derive(Debug, Serialize)]
#[cfg(target_os = "linux")]
pub(super) struct DirectoryRecord {
    pub(super) target: &'static str,
    pub(super) mount_id: u64,
    pub(super) parent_mount_id: u64,
    pub(super) device_major: u32,
    pub(super) device_minor: u32,
    pub(super) inode: u64,
    pub(super) uid: u32,
    pub(super) gid: u32,
    pub(super) mode: u32,
}

#[derive(Debug, Serialize)]
#[cfg(target_os = "linux")]
pub(super) struct FailureRecord {
    pub(super) schema: u8,
    pub(super) operation_id: String,
    pub(super) status: &'static str,
    pub(super) error: &'static str,
}

pub(super) fn valid_operation_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(any(target_os = "linux", test))]
fn validate_initial_root(uid: u32, gid: u32, empty: bool) -> Result<(), &'static str> {
    if uid != 0 || gid != 0 {
        return Err("target_owner");
    }
    if !empty {
        return Err("target_not_empty");
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn validate_mount_pair(home: u64, work: u64) -> Result<(), &'static str> {
    if home == work {
        Err("mounts_not_distinct")
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
#[cfg(any(target_os = "linux", test))]
struct MountIdentity {
    mount_id: u64,
    parent_mount_id: u64,
    device_major: u32,
    device_minor: u32,
    inode: u64,
}

#[cfg(any(target_os = "linux", test))]
fn validate_mount_identity(identity: MountIdentity) -> Result<(), &'static str> {
    if identity.mount_id == 0 || identity.parent_mount_id == 0 {
        return Err("target_mount_id");
    }
    if identity.mount_id == identity.parent_mount_id {
        return Err("target_not_mountpoint");
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn validate_distinct_roots(home: MountIdentity, work: MountIdentity) -> Result<(), &'static str> {
    validate_mount_pair(home.mount_id, work.mount_id)?;
    if home.device_major == work.device_major
        && home.device_minor == work.device_minor
        && home.inode == work.inode
    {
        return Err("mount_roots_not_distinct");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{
        DirectoryRecord, MountIdentity, ResultRecord, validate_distinct_roots,
        validate_initial_root, validate_mount_identity,
    };
    use rustix::{
        fs::{
            AtFlags, FileType, Gid, Mode, OFlags, StatxFlags, Uid, fchmod, fchown, fstat, open,
            openat, statx,
        },
        process::geteuid,
    };
    use std::{
        fs,
        os::fd::{AsFd, OwnedFd},
    };

    const HOME: &str = "/velnor/init/home";
    const WORK: &str = "/velnor/init/work";

    struct OpenDirectory {
        descriptor: OwnedFd,
        target: &'static str,
        identity: MountIdentity,
    }

    pub(super) fn initialize(operation_id: String) -> Result<ResultRecord, &'static str> {
        if geteuid().as_raw() != 0 {
            return Err("root_required");
        }
        let home = open_directory(HOME)?;
        let work = open_directory(WORK)?;
        validate_distinct_roots(home.identity, work.identity)?;
        validate_empty_root(&home)?;
        validate_empty_root(&work)?;
        set_runner_owner(&home)?;
        set_runner_owner(&work)?;
        Ok(ResultRecord {
            schema: 1,
            operation_id,
            home: verify_result(&home)?,
            work: verify_result(&work)?,
        })
    }

    fn open_directory(path: &'static str) -> Result<OpenDirectory, &'static str> {
        let (parent_path, name) = path.rsplit_once('/').ok_or("target_path")?;
        let parent = open_directory_path(parent_path)?;
        let descriptor = openat(
            &parent,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| "target_open")?;
        let parent_identity = statx(
            &parent,
            "",
            AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
            StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
        )
        .map_err(|_| "parent_mount")?;
        let identity = statx(
            &descriptor,
            "",
            AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
            StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
        )
        .map_err(|_| "target_mount")?;
        let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
        if parent_identity.stx_mask & required.bits() != required.bits()
            || identity.stx_mask & required.bits() != required.bits()
        {
            return Err("target_mount_fields");
        }
        let mount_identity = MountIdentity {
            mount_id: identity.stx_mnt_id,
            parent_mount_id: parent_identity.stx_mnt_id,
            device_major: identity.stx_dev_major,
            device_minor: identity.stx_dev_minor,
            inode: identity.stx_ino,
        };
        validate_mount_identity(mount_identity)?;
        if FileType::from_raw_mode(fstat(&descriptor).map_err(|_| "target_stat")?.st_mode)
            != FileType::Directory
        {
            return Err("target_type");
        }
        Ok(OpenDirectory {
            descriptor,
            target: path,
            identity: mount_identity,
        })
    }

    fn open_directory_path(path: &str) -> Result<OwnedFd, &'static str> {
        let root = open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| "root_open")?;
        let mut descriptor = root;
        for component in path.split('/').skip(1) {
            descriptor = openat(
                &descriptor,
                component,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| "parent_open")?;
        }
        Ok(descriptor)
    }

    fn validate_empty_root(directory: &OpenDirectory) -> Result<(), &'static str> {
        let status = fstat(&directory.descriptor).map_err(|_| "target_stat")?;
        let entries = fs::read_dir(format!(
            "/proc/self/fd/{}",
            directory.descriptor.as_fd().as_raw_fd()
        ))
        .map_err(|_| "target_read")?;
        validate_initial_root(
            status.st_uid,
            status.st_gid,
            entries.into_iter().next().is_none(),
        )
    }

    fn set_runner_owner(directory: &OpenDirectory) -> Result<(), &'static str> {
        fchown(
            &directory.descriptor,
            Some(Uid::from_raw(1000)),
            Some(Gid::from_raw(1000)),
        )
        .map_err(|_| "target_chown")?;
        fchmod(&directory.descriptor, Mode::from_raw_mode(0o755)).map_err(|_| "target_chmod")
    }

    fn verify_result(directory: &OpenDirectory) -> Result<DirectoryRecord, &'static str> {
        let status = fstat(&directory.descriptor).map_err(|_| "target_verify")?;
        let mode = status.st_mode & 0o7777;
        if status.st_uid != 1000 || status.st_gid != 1000 || mode != 0o755 {
            return Err("target_verify");
        }
        Ok(DirectoryRecord {
            target: directory.target,
            mount_id: directory.identity.mount_id,
            parent_mount_id: directory.identity.parent_mount_id,
            device_major: directory.identity.device_major,
            device_minor: directory.identity.device_minor,
            inode: directory.identity.inode,
            uid: status.st_uid,
            gid: status.st_gid,
            mode,
        })
    }

    use std::os::fd::AsRawFd;
}

#[cfg(target_os = "linux")]
pub(super) fn initialize(operation_id: String) -> Result<ResultRecord, &'static str> {
    linux::initialize(operation_id)
}

#[cfg(test)]
mod tests {
    use super::{
        MountIdentity, Request, valid_operation_id, validate_distinct_roots, validate_initial_root,
        validate_mount_identity, validate_mount_pair,
    };

    #[test]
    fn accepts_bounded_operation_identity() {
        assert!(valid_operation_id("a3b8-01"));
        assert!(valid_operation_id(&"a".repeat(64)));
    }

    #[test]
    fn rejects_invalid_operation_identity() {
        assert!(!valid_operation_id(""));
        assert!(!valid_operation_id(&"a".repeat(65)));
        assert!(!valid_operation_id("id/with/path"));
        assert!(!valid_operation_id("id with space"));
    }

    #[test]
    fn rejects_unknown_request_fields() {
        assert!(
            serde_json::from_str::<Request>(r#"{"operation_id":"abc","path":"/tmp"}"#).is_err()
        );
    }

    #[test]
    fn rejects_wrong_initial_owner_before_mutation() {
        assert_eq!(validate_initial_root(1000, 1000, true), Err("target_owner"));
        assert_eq!(validate_initial_root(0, 1000, true), Err("target_owner"));
    }

    #[test]
    fn rejects_nonempty_initial_root_before_mutation() {
        assert_eq!(validate_initial_root(0, 0, false), Err("target_not_empty"));
    }

    #[test]
    fn requires_separate_mounts_before_mutation() {
        assert_eq!(validate_mount_pair(27, 27), Err("mounts_not_distinct"));
        assert_eq!(validate_mount_pair(27, 28), Ok(()));
    }

    #[test]
    fn rejects_missing_or_zero_mount_identity() {
        assert_eq!(
            validate_mount_identity(MountIdentity {
                mount_id: 0,
                parent_mount_id: 1,
                device_major: 8,
                device_minor: 1,
                inode: 10,
            }),
            Err("target_mount_id")
        );
        assert_eq!(
            validate_mount_identity(MountIdentity {
                mount_id: 2,
                parent_mount_id: 0,
                device_major: 8,
                device_minor: 1,
                inode: 10,
            }),
            Err("target_mount_id")
        );
    }

    #[test]
    fn requires_target_mount_to_differ_from_parent_mount() {
        assert_eq!(
            validate_mount_identity(MountIdentity {
                mount_id: 2,
                parent_mount_id: 2,
                device_major: 8,
                device_minor: 1,
                inode: 10,
            }),
            Err("target_not_mountpoint")
        );
    }

    #[test]
    fn rejects_same_root_with_distinct_bind_mount_ids() {
        let root = MountIdentity {
            mount_id: 11,
            parent_mount_id: 10,
            device_major: 8,
            device_minor: 2,
            inode: 42,
        };
        let other_mount = MountIdentity {
            mount_id: 13,
            parent_mount_id: 10,
            ..root
        };
        assert_eq!(
            validate_distinct_roots(root, other_mount),
            Err("mount_roots_not_distinct")
        );
    }
}

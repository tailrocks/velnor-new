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
    pub(super) metadata: DirectoryRecord,
    pub(super) runtime_identity: DirectoryRecord,
    pub(super) job_facts: DirectoryRecord,
    pub(super) source_proof: DirectoryRecord,
    pub(super) control: DirectoryRecord,
    pub(super) inbox: DirectoryRecord,
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

#[cfg(any(target_os = "linux", test))]
fn validate_child_mount(parent_mount_id: u64, child_mount_id: u64) -> Result<(), &'static str> {
    if parent_mount_id == 0 || child_mount_id != parent_mount_id {
        return Err("child_mount_changed");
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn validate_directory_state(
    uid: u32,
    gid: u32,
    mode: u32,
    required_user: u32,
    required_group: u32,
    expected_mode: u32,
) -> Result<(), &'static str> {
    if uid != required_user || gid != required_group || mode != expected_mode {
        return Err("target_verify");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub(super) fn initialize(operation_id: String) -> Result<ResultRecord, &'static str> {
    linux::initialize(operation_id)
}

#[cfg(test)]
mod tests {
    use super::{
        MountIdentity, Request, valid_operation_id, validate_child_mount, validate_directory_state,
        validate_distinct_roots, validate_initial_root, validate_mount_identity,
        validate_mount_pair,
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

    #[test]
    fn metadata_children_must_remain_on_the_metadata_mount() {
        assert_eq!(validate_child_mount(41, 41), Ok(()));
        assert_eq!(validate_child_mount(41, 42), Err("child_mount_changed"));
        assert_eq!(validate_child_mount(0, 0), Err("child_mount_changed"));
    }

    #[test]
    fn metadata_child_directory_state_is_fixed() {
        assert_eq!(validate_directory_state(0, 0, 0o555, 0, 0, 0o555), Ok(()));
        assert_eq!(
            validate_directory_state(1000, 1000, 0o700, 1000, 1000, 0o700),
            Ok(())
        );
        assert_eq!(
            validate_directory_state(0, 1000, 0o555, 0, 0, 0o555),
            Err("target_verify")
        );
        assert_eq!(
            validate_directory_state(0, 0, 0o755, 0, 0, 0o555),
            Err("target_verify")
        );
    }
}

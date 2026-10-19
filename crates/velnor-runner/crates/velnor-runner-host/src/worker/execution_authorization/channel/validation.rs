use super::{ExecutionChannelDirectory, ProofChannelMount};

const CONTROL_TARGET: &str = "/opt/velnor/source-proof/control";
const INBOX_TARGET: &str = "/opt/velnor/source-proof/inbox";
pub(in crate::worker::execution_authorization) const JOB_FACTS_TARGET: &str =
    "/opt/velnor/job-facts";
const CONTROL_SUBPATH: &str = "source-proof/control";
const INBOX_SUBPATH: &str = "source-proof/inbox";
const JOB_FACTS_SUBPATH: &str = "job-facts";
const RUNNER_UID: u32 = 1000;
const ROOT_UID: u32 = 0;
const DIRECTORY_MODE: u32 = 0o555;
const INBOX_MODE: u32 = 0o700;

pub(in crate::worker::execution_authorization) fn valid_control_mount(
    mount: &ProofChannelMount,
) -> bool {
    mount.target == CONTROL_TARGET
        && mount.subpath == CONTROL_SUBPATH
        && mount.mount_id > 0
        && mount.fs_type_magic > 0
        && mount.filesystem_id > 0
        && mount.directory_inode > 0
        && valid_digest(&mount.filesystem_instance_digest)
        && mount.owner_uid == ROOT_UID
        && mount.owner_gid == ROOT_UID
        && mount.mode == DIRECTORY_MODE
        && mount.read_only
        && mount.no_copy
        && same_device_or_btrfs(mount)
}

pub(in crate::worker::execution_authorization) fn valid_inbox_mount(
    mount: &ProofChannelMount,
) -> bool {
    const TMPFS_MAGIC: u64 = 0x0102_1994;
    mount.target == INBOX_TARGET
        && mount.subpath == INBOX_SUBPATH
        && mount.mount_id > 0
        && mount.fs_type_magic > 0
        && mount.filesystem_id > 0
        && mount.directory_inode > 0
        && valid_digest(&mount.filesystem_instance_digest)
        && mount.owner_uid == RUNNER_UID
        && mount.owner_gid == RUNNER_UID
        && mount.mode == INBOX_MODE
        && mount.fs_type_magic == TMPFS_MAGIC
        && !mount.read_only
        && mount.no_copy
        && same_device_or_btrfs(mount)
}

pub(in crate::worker::execution_authorization) fn valid_job_facts_mount(
    mount: &ProofChannelMount,
) -> bool {
    mount.target == JOB_FACTS_TARGET
        && mount.subpath == JOB_FACTS_SUBPATH
        && mount.mount_id > 0
        && mount.fs_type_magic > 0
        && mount.filesystem_id > 0
        && mount.directory_inode > 0
        && valid_digest(&mount.filesystem_instance_digest)
        && mount.owner_uid == ROOT_UID
        && mount.owner_gid == ROOT_UID
        && mount.mode == DIRECTORY_MODE
        && mount.read_only
        && mount.no_copy
        && same_device_or_btrfs(mount)
}

pub(super) fn same_device_or_btrfs(mount: &ProofChannelMount) -> bool {
    (mount.device_major, mount.device_minor) == (mount.statx_device_major, mount.statx_device_minor)
        || mount.fs_type_magic == 0x9123_683e
}

pub(in crate::worker::execution_authorization) fn valid_engine(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

pub(in crate::worker::execution_authorization) fn valid_object_id(value: &str) -> bool {
    (12..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

pub(super) fn valid_operation_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn directory_matches_mount(
    directory: &ExecutionChannelDirectory,
    mount: &ProofChannelMount,
) -> bool {
    directory.target == mount.target
        && directory.inode == mount.directory_inode
        && directory.owner_uid == mount.owner_uid
        && directory.owner_gid == mount.owner_gid
        && directory.mode == mount.mode
        && directory.mount_id == mount.mount_id
        && directory.mount_device_major == mount.device_major
        && directory.mount_device_minor == mount.device_minor
        && directory.statx_device_major == mount.statx_device_major
        && directory.statx_device_minor == mount.statx_device_minor
        && directory.filesystem_type_magic == mount.fs_type_magic
        && directory.filesystem_id == mount.filesystem_id
        && directory.read_only == mount.read_only
}

pub(in crate::worker::execution_authorization) fn valid_volume_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

pub(in crate::worker::execution_authorization) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

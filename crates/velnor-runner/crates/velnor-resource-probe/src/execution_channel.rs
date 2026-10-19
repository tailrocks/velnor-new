//! Measure the specified Runner source-proof channel and read its specified request file.

use std::{fs::File, io::Read, os::unix::fs::MetadataExt};

use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, StatxFlags, fstat, fstatfs, fstatvfs, open, openat, statx,
};
use sha2::{Digest, Sha256};

use crate::{ProbeError, mountinfo::attachment_mount};

mod identity;
mod request;
#[cfg(test)]
mod tests;
mod wire;
use identity::{mount_namespace_id, read_boot_id};
use request::read_request_file;
pub use wire::{
    ExecutionChannelContext, ExecutionChannelDirectory, ExecutionChannelObservation,
    ExecutionRequestContext, ExecutionRequestRecord,
};

const CONTROL_PATH: &str = "/opt/velnor/source-proof/control";
const INBOX_PATH: &str = "/opt/velnor/source-proof/inbox";
pub(super) const REQUEST_FILE: &str = "request.json";
pub(super) const REQUEST_LIMIT: usize = 16 * 1024;
const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const MOUNTINFO_LIMIT: usize = 64 * 1024;
const CONTROL_UID: u32 = 0;
const CONTROL_GID: u32 = 0;
const RUNNER_UID: u32 = 1000;
const RUNNER_GID: u32 = 1000;

/// Measure both specified channel directories in this process mount namespace.
///
/// # Errors
///
/// Returns an error if the guest, channel, or boot identity does not have the specified value.
pub fn observe_execution_channel(
    context: ExecutionChannelContext,
) -> Result<ExecutionChannelObservation, ProbeError> {
    validate_context(context.schema_version, &context.operation_id)?;
    let boot_id = read_boot_id()?;
    if boot_id != context.expected_guest_boot_id {
        return Err(ProbeError::Invalid("execution_channel_boot_id"));
    }
    let mountinfo = read_bounded(
        MOUNTINFO_PATH,
        MOUNTINFO_LIMIT,
        "execution_channel_mountinfo",
    )?;
    let control = observe_directory(CONTROL_PATH, CONTROL_UID, CONTROL_GID, 0o555, &mountinfo)?;
    let inbox = observe_directory(INBOX_PATH, RUNNER_UID, RUNNER_GID, 0o700, &mountinfo)?;
    if control.mount_id == inbox.mount_id
        || control.inode == inbox.inode
        || control.filesystem_id != inbox.filesystem_id
        || control.filesystem_type_magic != inbox.filesystem_type_magic
        || control.mount_device_major != inbox.mount_device_major
        || control.mount_device_minor != inbox.mount_device_minor
        || control.statx_device_major != inbox.statx_device_major
        || control.statx_device_minor != inbox.statx_device_minor
    {
        return Err(ProbeError::Invalid("execution_channel_identity"));
    }
    let namespace = mount_namespace_id()?;
    if read_bounded(
        MOUNTINFO_PATH,
        MOUNTINFO_LIMIT,
        "execution_channel_mountinfo",
    )? != mountinfo
        || observe_directory(CONTROL_PATH, CONTROL_UID, CONTROL_GID, 0o555, &mountinfo)? != control
        || observe_directory(INBOX_PATH, RUNNER_UID, RUNNER_GID, 0o700, &mountinfo)? != inbox
        || read_boot_id()? != boot_id
        || mount_namespace_id()? != namespace
    {
        return Err(ProbeError::Invalid("execution_channel_changed"));
    }
    Ok(ExecutionChannelObservation {
        schema_version: 1,
        operation_id: context.operation_id,
        guest_boot_id: boot_id,
        mount_namespace_id: namespace,
        control,
        inbox,
    })
}

/// Read request bytes from the specified inbox directory.
///
/// # Errors
///
/// Returns an error if the channel changes or the request file is not safe.
pub fn read_execution_request(
    context: ExecutionRequestContext,
) -> Result<ExecutionRequestRecord, ProbeError> {
    validate_context(context.schema_version, &context.operation_id)?;
    if context.expected_channel.operation_id != context.operation_id
        || context.expected_channel.guest_boot_id != context.expected_guest_boot_id
    {
        return Err(ProbeError::Invalid("execution_request_context"));
    }
    let current = observe_execution_channel(ExecutionChannelContext {
        schema_version: context.schema_version,
        operation_id: context.operation_id.clone(),
        expected_guest_boot_id: context.expected_guest_boot_id.clone(),
    })?;
    if current != context.expected_channel {
        return Err(ProbeError::Invalid("execution_request_channel_changed"));
    }
    let inbox = open_fixed_directory(INBOX_PATH)?;
    let mountinfo_before = read_bounded(
        MOUNTINFO_PATH,
        MOUNTINFO_LIMIT,
        "execution_channel_mountinfo",
    )?;
    let inbox_before = observe_open_directory(
        &inbox,
        INBOX_PATH,
        RUNNER_UID,
        RUNNER_GID,
        0o700,
        &mountinfo_before,
    )?;
    ensure_channel_directory(&current.inbox, &inbox_before)?;
    let (bytes, after) = read_request_file(&inbox, RUNNER_UID, RUNNER_GID)?;
    let mountinfo_after = read_bounded(
        MOUNTINFO_PATH,
        MOUNTINFO_LIMIT,
        "execution_channel_mountinfo",
    )?;
    let inbox_after = observe_open_directory(
        &inbox,
        INBOX_PATH,
        RUNNER_UID,
        RUNNER_GID,
        0o700,
        &mountinfo_after,
    )?;
    let channel_after = observe_execution_channel(ExecutionChannelContext {
        schema_version: context.schema_version,
        operation_id: context.operation_id.clone(),
        expected_guest_boot_id: context.expected_guest_boot_id.clone(),
    })?;
    ensure_channel_directory(&inbox_before, &inbox_after)?;
    if channel_after != current {
        return Err(ProbeError::Invalid("execution_request_channel_changed"));
    }
    let sha256 = lower_hex(&Sha256::digest(&bytes));
    Ok(ExecutionRequestRecord {
        schema_version: 1,
        operation_id: context.operation_id,
        guest_boot_id: current.guest_boot_id.clone(),
        channel: current,
        bytes_hex: lower_hex(&bytes),
        sha256,
        inode: after.inode,
        owner_uid: after.user_id,
        owner_gid: after.group_id,
        mode: after.mode,
        size: after.size,
    })
}

fn observe_directory(
    path: &str,
    user_id: u32,
    group_id: u32,
    expected_mode: u32,
    mountinfo: &[u8],
) -> Result<ExecutionChannelDirectory, ProbeError> {
    let file = open_fixed_directory(path)?;
    observe_open_directory(&file, path, user_id, group_id, expected_mode, mountinfo)
}

fn ensure_channel_directory(
    expected: &ExecutionChannelDirectory,
    observed: &ExecutionChannelDirectory,
) -> Result<(), ProbeError> {
    if expected == observed {
        Ok(())
    } else {
        Err(ProbeError::Invalid("execution_request_channel_changed"))
    }
}

fn observe_open_directory(
    file: &File,
    path: &str,
    user_id: u32,
    group_id: u32,
    expected_mode: u32,
    mountinfo: &[u8],
) -> Result<ExecutionChannelDirectory, ProbeError> {
    let metadata = file
        .metadata()
        .map_err(|error| ProbeError::Read("execution_channel_stat", error))?;
    let mode = metadata.mode() & 0o777;
    if !metadata.is_dir()
        || metadata.uid() != user_id
        || metadata.gid() != group_id
        || mode != expected_mode
    {
        return Err(ProbeError::Invalid("execution_channel_directory"));
    }
    let stat =
        fstat(file).map_err(|error| ProbeError::Read("execution_channel_fstat", error.into()))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        return Err(ProbeError::Invalid("execution_channel_directory"));
    }
    let statx_value = statx(
        file,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|error| ProbeError::Read("execution_channel_statx", error.into()))?;
    let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
    if statx_value.stx_mask & required.bits() != required.bits() {
        return Err(ProbeError::Invalid("execution_channel_statx_fields"));
    }
    let fs = fstatfs(file)
        .map_err(|error| ProbeError::Read("execution_channel_statfs", error.into()))?;
    let vfs = fstatvfs(file)
        .map_err(|error| ProbeError::Read("execution_channel_statvfs", error.into()))?;
    let mount = attachment_mount(mountinfo, path)?;
    let filesystem_type_magic =
        u64::try_from(fs.f_type).map_err(|_| ProbeError::Invalid("execution_channel_fs_type"))?;
    let btrfs = filesystem_type_magic == crate::filesystem::BTRFS_SUPER_MAGIC;
    if mount.mount_id != statx_value.stx_mnt_id
        || (!btrfs
            && (mount.device_major != statx_value.stx_dev_major
                || mount.device_minor != statx_value.stx_dev_minor))
        || btrfs != (mount.filesystem_type == "btrfs")
    {
        return Err(ProbeError::Invalid("execution_channel_mount_identity"));
    }
    if (path == CONTROL_PATH && !mount.read_only) || (path == INBOX_PATH && mount.read_only) {
        return Err(ProbeError::Invalid("execution_channel_mount_mode"));
    }
    Ok(ExecutionChannelDirectory {
        target: path.to_owned(),
        inode: metadata.ino(),
        owner_uid: metadata.uid(),
        owner_gid: metadata.gid(),
        mode,
        mount_id: mount.mount_id,
        statx_mount_id: statx_value.stx_mnt_id,
        mount_device_major: mount.device_major,
        mount_device_minor: mount.device_minor,
        statx_device_major: statx_value.stx_dev_major,
        statx_device_minor: statx_value.stx_dev_minor,
        filesystem_type_magic,
        filesystem_id: vfs.f_fsid,
        read_only: mount.read_only,
        executable_mount: mount.executable,
    })
}

fn open_fixed_directory(path: &str) -> Result<File, ProbeError> {
    let mut current = File::from(
        open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| ProbeError::Read("execution_channel_root", error.into()))?,
    );
    let relative = path
        .strip_prefix('/')
        .ok_or(ProbeError::Invalid("execution_channel_path"))?;
    for component in relative.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(ProbeError::Invalid("execution_channel_path"));
        }
        current = File::from(
            openat(
                &current,
                component,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| ProbeError::Read("execution_channel_open", error.into()))?,
        );
    }
    Ok(current)
}

fn read_bounded(path: &str, limit: usize, name: &'static str) -> Result<Vec<u8>, ProbeError> {
    let file = File::open(path).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}

fn validate_context(schema_version: u8, operation_id: &str) -> Result<(), ProbeError> {
    if schema_version != 1
        || operation_id.len() != 32
        || !operation_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProbeError::Invalid("execution_channel_context"));
    }
    Ok(())
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

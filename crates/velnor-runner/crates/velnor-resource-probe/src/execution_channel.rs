//! Measure the specified Runner source-proof channel and read its specified request file.

use std::{fs::File, io::Read, os::unix::fs::MetadataExt};

use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, StatxFlags, fstat, fstatfs, fstatvfs, open, openat, statx,
};
use sha2::{Digest, Sha256};

use crate::{
    ProbeError,
    filesystem::filesystem_name,
    mountinfo::{attachment_mount, attachment_mount_source},
};

pub(crate) mod identity;
mod request;
#[cfg(test)]
mod tests;
pub use crate::execution_channel_wire::{
    ExecutionChannelContext, ExecutionChannelDirectory, ExecutionChannelObservation,
    ExecutionRequestContext, ExecutionRequestOutcome, ExecutionRequestRecord,
};
use identity::{mount_namespace_id, read_boot_id};
use request::read_request_file;

const CONTROL_PATH: &str = "/opt/velnor/source-proof/control";
const INBOX_PATH: &str = "/opt/velnor/source-proof/inbox";
const JOB_FACTS_PATH: &str = "/opt/velnor/job-facts";
pub(super) const REQUEST_FILE: &str = "request.json";
pub(super) const REQUEST_LIMIT: usize = 16 * 1024;
const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const MOUNTINFO_LIMIT: usize = 64 * 1024;
const CONTROL_OWNER: (u32, u32) = (0, 0);
const CONTROL_MODE: u32 = 0o555;
const RUNNER_OWNER: (u32, u32) = (1000, 1000);
const INBOX_MODE: u32 = 0o700;

/// Measure the channel and job facts directories in this process mount namespace.
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
    let control = observe_directory(CONTROL_PATH, &mountinfo)?;
    let inbox = observe_directory(INBOX_PATH, &mountinfo)?;
    let job_facts = observe_directory(JOB_FACTS_PATH, &mountinfo)?;
    ensure_distinct_mounts(&control, &inbox)?;
    ensure_distinct_mounts(&control, &job_facts)?;
    ensure_distinct_mounts(&inbox, &job_facts)?;
    let namespace = mount_namespace_id()?;
    if read_bounded(
        MOUNTINFO_PATH,
        MOUNTINFO_LIMIT,
        "execution_channel_mountinfo",
    )? != mountinfo
        || observe_directory(CONTROL_PATH, &mountinfo)? != control
        || observe_directory(INBOX_PATH, &mountinfo)? != inbox
        || observe_directory(JOB_FACTS_PATH, &mountinfo)? != job_facts
        || read_boot_id()? != boot_id
        || mount_namespace_id()? != namespace
    {
        return Err(ProbeError::Invalid("execution_channel_changed"));
    }
    Ok(ExecutionChannelObservation {
        schema_version: 3,
        operation_id: context.operation_id,
        guest_boot_id: boot_id,
        mount_namespace_id: namespace,
        control,
        inbox,
        job_facts,
    })
}

fn ensure_distinct_mounts(
    control: &ExecutionChannelDirectory,
    inbox: &ExecutionChannelDirectory,
) -> Result<(), ProbeError> {
    let same_backing_object = control.inode == inbox.inode
        && control.filesystem_id == inbox.filesystem_id
        && control.filesystem_type_magic == inbox.filesystem_type_magic
        && control.statx_device_major == inbox.statx_device_major
        && control.statx_device_minor == inbox.statx_device_minor
        && control.mount_root == inbox.mount_root;
    if control.mount_id == inbox.mount_id || same_backing_object {
        Err(ProbeError::Invalid("execution_channel_identity"))
    } else {
        Ok(())
    }
}

/// Read request bytes from the specified inbox directory.
///
/// # Errors
///
/// Returns an error if the channel changes or the request file is not safe.
pub fn read_execution_request(
    context: ExecutionRequestContext,
) -> Result<ExecutionRequestOutcome, ProbeError> {
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
    let inbox_before = observe_open_directory(&inbox, INBOX_PATH, &mountinfo_before)?;
    ensure_channel_directory(&current.inbox, &inbox_before)?;
    let request = read_request_file(&inbox, RUNNER_OWNER.0, RUNNER_OWNER.1)?;
    let mountinfo_after = read_bounded(
        MOUNTINFO_PATH,
        MOUNTINFO_LIMIT,
        "execution_channel_mountinfo",
    )?;
    let inbox_after = observe_open_directory(&inbox, INBOX_PATH, &mountinfo_after)?;
    let channel_after = observe_execution_channel(ExecutionChannelContext {
        schema_version: context.schema_version,
        operation_id: context.operation_id.clone(),
        expected_guest_boot_id: context.expected_guest_boot_id.clone(),
    })?;
    ensure_channel_directory(&inbox_before, &inbox_after)?;
    if channel_after != current {
        return Err(ProbeError::Invalid("execution_request_channel_changed"));
    }
    let Some((bytes, after)) = request else {
        return Ok(ExecutionRequestOutcome::NotYetPresent {
            schema_version: 3,
            operation_id: context.operation_id,
            guest_boot_id: current.guest_boot_id.clone(),
            channel: current,
        });
    };
    let sha256 = lower_hex(&Sha256::digest(&bytes));
    Ok(ExecutionRequestOutcome::Present {
        record: ExecutionRequestRecord {
            schema_version: 3,
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
        },
    })
}

fn observe_directory(
    path: &str,
    mountinfo: &[u8],
) -> Result<ExecutionChannelDirectory, ProbeError> {
    let file = open_fixed_directory(path)?;
    observe_open_directory(&file, path, mountinfo)
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
    mountinfo: &[u8],
) -> Result<ExecutionChannelDirectory, ProbeError> {
    let (metadata, statx_value, mode) = opened_directory_identity(file)?;
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
    validate_directory_contract(path, (metadata.uid(), metadata.gid()), mode, &mount)?;
    let mount_source = attachment_mount_source(mountinfo, path)?;
    let filesystem_type_name = filesystem_name(&mount, filesystem_type_magic)?;
    Ok(ExecutionChannelDirectory {
        target: path.to_owned(),
        inode: metadata.ino(),
        owner_uid: metadata.uid(),
        owner_gid: metadata.gid(),
        mode,
        mount_id: mount.mount_id,
        statx_mount_id: statx_value.stx_mnt_id,
        mount_root: mount.root,
        mount_source,
        mount_device_major: mount.device_major,
        mount_device_minor: mount.device_minor,
        statx_device_major: statx_value.stx_dev_major,
        statx_device_minor: statx_value.stx_dev_minor,
        filesystem_type_magic,
        filesystem_type_name,
        filesystem_id: vfs.f_fsid,
        read_only: mount.read_only,
        executable_mount: mount.executable,
    })
}

fn opened_directory_identity(
    file: &File,
) -> Result<(std::fs::Metadata, rustix::fs::Statx, u32), ProbeError> {
    let metadata = file
        .metadata()
        .map_err(|error| ProbeError::Read("execution_channel_stat", error))?;
    let mode = metadata.mode() & 0o7777;
    if !metadata.is_dir() {
        return Err(ProbeError::Invalid("execution_channel_directory"));
    }
    let stat =
        fstat(file).map_err(|error| ProbeError::Read("execution_channel_fstat", error.into()))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_ino != metadata.ino()
        || stat.st_uid != metadata.uid()
        || stat.st_gid != metadata.gid()
    {
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
    if u32::from(statx_value.stx_mode) != stat.st_mode
        || statx_value.stx_ino != metadata.ino()
        || statx_value.stx_uid != metadata.uid()
        || statx_value.stx_gid != metadata.gid()
    {
        return Err(ProbeError::Invalid("execution_channel_statx_identity"));
    }
    Ok((metadata, statx_value, mode))
}

fn validate_directory_contract(
    path: &str,
    owner: (u32, u32),
    actual_mode: u32,
    mount: &crate::mountinfo::MountEntry,
) -> Result<(), ProbeError> {
    let expected = match path {
        CONTROL_PATH | JOB_FACTS_PATH => (CONTROL_OWNER, CONTROL_MODE, true),
        INBOX_PATH => (RUNNER_OWNER, INBOX_MODE, false),
        _ => return Err(ProbeError::Invalid("execution_channel_path")),
    };
    if owner != expected.0 || actual_mode != expected.1 || mount.read_only != expected.2 {
        return Err(ProbeError::Invalid("execution_channel_directory_contract"));
    }
    Ok(())
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
    if schema_version != 3
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

mod observation;

use observation::{
    decode_payload, lower_hex, observe_control_directory, observe_control_file_identity,
    open_control_directory, read_mountinfo, valid_boot_id, valid_digest, valid_operation_id,
};

use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
};

use rustix::{
    fs::{
        AtFlags, FileType, Gid, Mode, OFlags, RenameFlags, StatxFlags, Uid, fchmod, fchown, fstat,
        fstatfs, fstatvfs, open, openat, renameat_with, statx,
    },
    process::geteuid,
};
use sha2::{Digest, Sha256};

use crate::{
    ProbeError,
    execution_channel_wire::ExecutionChannelDirectory,
    filesystem::filesystem_name,
    mountinfo::{MountEntry, attachment_mount, attachment_mount_source},
};

use super::{
    ExecutionPublicationContext, ExecutionPublicationFile, ExecutionPublicationKind,
    ExecutionPublicationRecord,
};

const PUBLISHER_ROOT: &str = "/opt/velnor";
const CONTROL_PATH: &str = "/opt/velnor/source-proof/control";
const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const MOUNTINFO_LIMIT: usize = 64 * 1024;
const CONTROL_UID: u32 = 0;
const CONTROL_GID: u32 = 0;
const CONTROL_DIRECTORY_MODE: u32 = 0o555;
const CONTROL_FILE_MODE: u32 = 0o444;
const CONTROL_FILE_LIMIT: usize = 8 * 1024;
const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectoryIdentity {
    inode: u64,
    owner_uid: u32,
    owner_gid: u32,
    mode: u32,
    mount_id: u64,
    statx_mount_id: u64,
    mount_device_major: u32,
    mount_device_minor: u32,
    statx_device_major: u32,
    statx_device_minor: u32,
    filesystem_type_magic: u64,
    filesystem_type_name: String,
    filesystem_id: u64,
    mount_root: String,
    mount_source: String,
    read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenedControlFile {
    bytes: Vec<u8>,
    identity: FileIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileIdentity {
    inode: u64,
    owner_uid: u32,
    owner_gid: u32,
    mode: u32,
    size: u64,
    mount_id: u64,
    statx_mount_id: u64,
    mount_device_major: u32,
    mount_device_minor: u32,
    statx_device_major: u32,
    statx_device_minor: u32,
    filesystem_type_magic: u64,
    filesystem_id: u64,
    mount_root: String,
    mount_source: String,
}

pub(super) fn publish_execution_control(
    context: ExecutionPublicationContext,
) -> Result<ExecutionPublicationRecord, ProbeError> {
    validate_request(&context)?;
    if geteuid().as_raw() != CONTROL_UID {
        return Err(ProbeError::Invalid("publication_root_required"));
    }
    let bytes = decode_payload(&context.payload_hex)?;
    let boot_id = crate::execution_channel::identity::read_boot_id()?;
    if boot_id != context.expected_guest_boot_id {
        return Err(ProbeError::Invalid("publication_boot_id"));
    }
    let namespace = crate::execution_channel::identity::mount_namespace_id()?;
    let mountinfo = read_mountinfo()?;
    let root_mount = attachment_mount(&mountinfo, PUBLISHER_ROOT)?;
    if root_mount.read_only {
        return Err(ProbeError::Invalid("publication_mount_read_only"));
    }
    let control = open_control_directory()?;
    let directory_before = observe_control_directory(&control, &root_mount, &mountinfo)?;
    compare_runner_directory(&context.expected_runner_control, &directory_before)?;
    let published = publish_or_read(
        &control,
        context.kind,
        &context.operation_id,
        &bytes,
        &root_mount,
        &mountinfo,
    )?;
    let file_before = open_control_file(&control, context.kind, &root_mount, &mountinfo)?;
    let mountinfo_after = read_mountinfo()?;
    let root_mount_after = attachment_mount(&mountinfo_after, PUBLISHER_ROOT)?;
    let directory_after = observe_control_directory(&control, &root_mount_after, &mountinfo_after)?;
    let final_opened =
        open_control_file(&control, context.kind, &root_mount_after, &mountinfo_after)?;
    let boot_after = crate::execution_channel::identity::read_boot_id()?;
    let namespace_after = crate::execution_channel::identity::mount_namespace_id()?;
    if mountinfo_after != mountinfo
        || root_mount_after != root_mount
        || directory_after != directory_before
        || final_opened != file_before
        || file_before != published
        || boot_after != boot_id
        || namespace_after != namespace
    {
        return Err(ProbeError::Invalid("publication_state_changed"));
    }
    Ok(ExecutionPublicationRecord {
        schema_version: 1,
        operation_id: context.operation_id,
        guest_boot_id: boot_id,
        mount_namespace_id: namespace,
        kind: context.kind,
        payload_sha256: context.payload_sha256,
        file: publication_file(
            context.kind,
            directory_before.inode,
            file_before.identity,
            &file_before.bytes,
        ),
    })
}

fn validate_request(context: &ExecutionPublicationContext) -> Result<(), ProbeError> {
    if context.schema_version != 1
        || !valid_operation_id(&context.operation_id)
        || !valid_boot_id(&context.expected_guest_boot_id)
        || !valid_digest(&context.payload_sha256)
        || context.payload_hex.len() > CONTROL_FILE_LIMIT * 2
        || !context.payload_hex.len().is_multiple_of(2)
    {
        return Err(ProbeError::Invalid("publication_request"));
    }
    let expected = &context.expected_runner_control;
    if expected.target != CONTROL_PATH
        || expected.inode == 0
        || expected.owner_uid != CONTROL_UID
        || expected.owner_gid != CONTROL_GID
        || expected.mode != CONTROL_DIRECTORY_MODE
        || !expected.read_only
        || expected.mount_id == 0
        || expected.statx_mount_id == 0
        || expected.filesystem_type_magic == 0
        || expected.filesystem_id == 0
    {
        return Err(ProbeError::Invalid("publication_runner_binding"));
    }
    let bytes = decode_payload(&context.payload_hex)?;
    let digest = lower_hex(&Sha256::digest(&bytes));
    if bytes.is_empty() || digest != context.payload_sha256 {
        return Err(ProbeError::Invalid("publication_payload_digest"));
    }
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|_| ProbeError::Invalid("publication_payload_json"))?;
    Ok(())
}

fn compare_runner_directory(
    expected: &ExecutionChannelDirectory,
    observed: &DirectoryIdentity,
) -> Result<(), ProbeError> {
    let btrfs = observed.filesystem_type_magic == BTRFS_SUPER_MAGIC;
    if observed.inode != expected.inode
        || observed.owner_uid != expected.owner_uid
        || observed.owner_gid != expected.owner_gid
        || observed.mode != expected.mode
        || observed.filesystem_type_magic != expected.filesystem_type_magic
        || observed.filesystem_type_name != expected.filesystem_type_name
        || observed.filesystem_id != expected.filesystem_id
        || observed.mount_source != expected.mount_source
        || observed.mount_device_major != expected.mount_device_major
        || observed.mount_device_minor != expected.mount_device_minor
        || (!btrfs
            && (observed.statx_device_major != expected.statx_device_major
                || observed.statx_device_minor != expected.statx_device_minor))
        || observed.read_only
    {
        return Err(ProbeError::Invalid("publication_channel_binding"));
    }
    Ok(())
}

fn publish_or_read(
    directory: &File,
    kind: ExecutionPublicationKind,
    operation_id: &str,
    bytes: &[u8],
    mount: &MountEntry,
    mountinfo: &[u8],
) -> Result<OpenedControlFile, ProbeError> {
    match openat(
        directory,
        kind.file_name(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => {
            let opened = read_control_file(&File::from(fd), directory, mount, mountinfo)?;
            if opened.bytes != bytes {
                return Err(ProbeError::Invalid("publication_replay_conflict"));
            }
            return Ok(opened);
        }
        Err(rustix::io::Errno::NOENT) => {}
        Err(_) => return Err(ProbeError::Invalid("publication_destination_open")),
    }
    let temporary = format!(".{}.{}.tmp", kind.label(), operation_id);
    let fd = openat(
        directory,
        temporary.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| ProbeError::Invalid("publication_temporary_create"))?;
    let mut file = File::from(fd);
    file.write_all(bytes)
        .map_err(|_| ProbeError::Invalid("publication_write"))?;
    fchown(
        &file,
        Some(Uid::from_raw(CONTROL_UID)),
        Some(Gid::from_raw(CONTROL_GID)),
    )
    .map_err(|_| ProbeError::Invalid("publication_owner"))?;
    fchmod(&file, Mode::from_raw_mode(CONTROL_FILE_MODE))
        .map_err(|_| ProbeError::Invalid("publication_mode"))?;
    file.sync_all()
        .map_err(|_| ProbeError::Invalid("publication_file_sync"))?;
    renameat_with(
        directory,
        temporary.as_str(),
        directory,
        kind.file_name(),
        RenameFlags::NOREPLACE,
    )
    .map_err(|_| ProbeError::Invalid("publication_rename"))?;
    directory
        .sync_all()
        .map_err(|_| ProbeError::Invalid("publication_directory_sync"))?;
    open_control_file(directory, kind, mount, mountinfo)
}

fn open_control_file(
    directory: &File,
    kind: ExecutionPublicationKind,
    mount: &MountEntry,
    mountinfo: &[u8],
) -> Result<OpenedControlFile, ProbeError> {
    let fd = openat(
        directory,
        kind.file_name(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ProbeError::Invalid("publication_destination_open"))?;
    read_control_file(&File::from(fd), directory, mount, mountinfo)
}

fn read_control_file(
    file: &File,
    directory: &File,
    mount_entry: &MountEntry,
    mountinfo: &[u8],
) -> Result<OpenedControlFile, ProbeError> {
    let metadata = file
        .metadata()
        .map_err(|_| ProbeError::Invalid("publication_file_stat"))?;
    let mode = metadata.mode() & 0o777;
    if !metadata.is_file()
        || metadata.uid() != CONTROL_UID
        || metadata.gid() != CONTROL_GID
        || mode != CONTROL_FILE_MODE
        || metadata.len() == 0
        || metadata.len() > CONTROL_FILE_LIMIT as u64
    {
        return Err(ProbeError::Invalid("publication_file_identity"));
    }
    let bytes = read_control_bytes(file)?;
    let identity = observe_control_file_identity(
        file,
        directory,
        mount_entry,
        mountinfo,
        &metadata,
        bytes.len(),
    )?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|_| ProbeError::Invalid("publication_file_json"))?;
    Ok(OpenedControlFile { bytes, identity })
}

fn read_control_bytes(file: &File) -> Result<Vec<u8>, ProbeError> {
    let mut bytes = Vec::with_capacity(CONTROL_FILE_LIMIT.min(4096));
    file.try_clone()
        .map_err(|_| ProbeError::Invalid("publication_file_clone"))?
        .take((CONTROL_FILE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProbeError::Invalid("publication_file_read"))?;
    if bytes.len() > CONTROL_FILE_LIMIT {
        return Err(ProbeError::InputTooLarge("publication_file"));
    }
    Ok(bytes)
}

fn publication_file(
    kind: ExecutionPublicationKind,
    control_directory_inode: u64,
    identity: FileIdentity,
    bytes: &[u8],
) -> ExecutionPublicationFile {
    ExecutionPublicationFile {
        path: format!("{CONTROL_PATH}/{}", kind.file_name()),
        control_directory_inode,
        inode: identity.inode,
        bytes: identity.size,
        owner_uid: identity.owner_uid,
        owner_gid: identity.owner_gid,
        mode: identity.mode,
        mount_id: identity.mount_id,
        statx_mount_id: identity.statx_mount_id,
        mount_root: identity.mount_root,
        mount_source: identity.mount_source,
        mount_device_major: identity.mount_device_major,
        mount_device_minor: identity.mount_device_minor,
        statx_device_major: identity.statx_device_major,
        statx_device_minor: identity.statx_device_minor,
        filesystem_type_magic: identity.filesystem_type_magic,
        filesystem_id: identity.filesystem_id,
        readback_sha256: lower_hex(&Sha256::digest(bytes)),
    }
}

#[cfg(test)]
#[path = "publish/tests.rs"]
mod tests;

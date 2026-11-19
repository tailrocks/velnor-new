// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod files;
mod observation;

use files::{open_control_file, read_control_file};

use observation::{
    decode_payload, lower_hex, observe_control_directory, open_control_directory, read_mountinfo,
    valid_boot_id, valid_digest, valid_operation_id,
};

use std::{fs::File, io::Write, os::unix::fs::MetadataExt};

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
const CONTROL_UID: u32 = 0;
const CONTROL_GID: u32 = 0;
const CONTROL_DIRECTORY_MODE: u32 = 0o555;
const CONTROL_FILE_MODE: u32 = 0o444;
const CONTROL_FILE_LIMIT: usize = 8 * 1024;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExpectedOwner {
    uid: u32,
    gid: u32,
}

const CONTROL_OWNER: ExpectedOwner = ExpectedOwner {
    uid: CONTROL_UID,
    gid: CONTROL_GID,
};

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
    let file_before = open_control_file(
        &control,
        context.kind,
        &root_mount,
        &mountinfo,
        CONTROL_OWNER,
    )?;
    let mountinfo_after = read_mountinfo()?;
    let root_mount_after = attachment_mount(&mountinfo_after, PUBLISHER_ROOT)?;
    let directory_after = observe_control_directory(&control, &root_mount_after, &mountinfo_after)?;
    let final_opened = open_control_file(
        &control,
        context.kind,
        &root_mount_after,
        &mountinfo_after,
        CONTROL_OWNER,
    )?;
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
        || observed.statx_device_major != expected.statx_device_major
        || observed.statx_device_minor != expected.statx_device_minor
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
    publish_or_read_with_owner(
        directory,
        kind,
        operation_id,
        bytes,
        mount,
        mountinfo,
        CONTROL_OWNER,
    )
}

fn publish_or_read_with_owner(
    directory: &File,
    kind: ExecutionPublicationKind,
    operation_id: &str,
    bytes: &[u8],
    mount: &MountEntry,
    mountinfo: &[u8],
    expected_owner: ExpectedOwner,
) -> Result<OpenedControlFile, ProbeError> {
    match openat(
        directory,
        kind.file_name(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => {
            let opened =
                read_control_file(&File::from(fd), directory, mount, mountinfo, expected_owner)?;
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
        Some(Uid::from_raw(expected_owner.uid)),
        Some(Gid::from_raw(expected_owner.gid)),
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
    open_control_file(directory, kind, mount, mountinfo, expected_owner)
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
mod btrfs_binding_tests {
    use super::*;

    fn expected(statx_minor: u32) -> ExecutionChannelDirectory {
        ExecutionChannelDirectory {
            target: "/opt/velnor/control".to_owned(),
            inode: 256,
            owner_uid: CONTROL_UID,
            owner_gid: CONTROL_GID,
            mode: CONTROL_DIRECTORY_MODE,
            mount_id: 7,
            statx_mount_id: 7,
            mount_root: "/".to_owned(),
            mount_source: "/dev/vda1".to_owned(),
            mount_device_major: 0,
            mount_device_minor: 40,
            statx_device_major: 0,
            statx_device_minor: statx_minor,
            filesystem_type_magic: crate::filesystem::BTRFS_SUPER_MAGIC,
            filesystem_type_name: "btrfs".to_owned(),
            filesystem_id: 9,
            read_only: false,
            executable_mount: false,
        }
    }

    fn observed(statx_minor: u32) -> DirectoryIdentity {
        let e = expected(statx_minor);
        DirectoryIdentity {
            inode: e.inode,
            owner_uid: e.owner_uid,
            owner_gid: e.owner_gid,
            mode: e.mode,
            mount_id: e.mount_id,
            statx_mount_id: e.statx_mount_id,
            mount_device_major: e.mount_device_major,
            mount_device_minor: e.mount_device_minor,
            statx_device_major: e.statx_device_major,
            statx_device_minor: e.statx_device_minor,
            filesystem_type_magic: e.filesystem_type_magic,
            filesystem_type_name: e.filesystem_type_name,
            filesystem_id: e.filesystem_id,
            mount_root: e.mount_root,
            mount_source: e.mount_source,
            read_only: false,
        }
    }

    #[test]
    fn btrfs_same_subvolume_is_accepted() {
        assert!(compare_runner_directory(&expected(41), &observed(41)).is_ok());
    }

    #[test]
    fn btrfs_other_subvolume_with_same_inode_is_refused() {
        assert!(compare_runner_directory(&expected(41), &observed(42)).is_err());
    }
}

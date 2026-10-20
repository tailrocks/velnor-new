use std::{io::Read, os::unix::fs::MetadataExt};

use rustix::fs::{Mode, OFlags, openat};

use super::{
    CONTROL_FILE_LIMIT, CONTROL_FILE_MODE, ExecutionPublicationKind, File, MountEntry,
    OpenedControlFile, ProbeError, observation::observe_control_file_identity,
};

pub(super) fn open_control_file(
    directory: &File,
    kind: ExecutionPublicationKind,
    mount: &MountEntry,
    mountinfo: &[u8],
    owner_uid: u32,
    owner_gid: u32,
) -> Result<OpenedControlFile, ProbeError> {
    let fd = openat(
        directory,
        kind.file_name(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ProbeError::Invalid("publication_destination_open"))?;
    read_control_file(
        &File::from(fd),
        directory,
        mount,
        mountinfo,
        owner_uid,
        owner_gid,
    )
}

pub(super) fn read_control_file(
    file: &File,
    directory: &File,
    mount_entry: &MountEntry,
    mountinfo: &[u8],
    owner_uid: u32,
    owner_gid: u32,
) -> Result<OpenedControlFile, ProbeError> {
    let metadata = file
        .metadata()
        .map_err(|_| ProbeError::Invalid("publication_file_stat"))?;
    let mode = metadata.mode() & 0o777;
    if !metadata.is_file()
        || metadata.uid() != owner_uid
        || metadata.gid() != owner_gid
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
        owner_uid,
        owner_gid,
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

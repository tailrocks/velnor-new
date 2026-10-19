use std::{fs::File, io::Read, os::unix::fs::MetadataExt};

use rustix::fs::{Mode, OFlags, openat};

use crate::ProbeError;

use super::{REQUEST_FILE, REQUEST_LIMIT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RequestMetadata {
    pub(super) inode: u64,
    pub(super) user_id: u32,
    pub(super) group_id: u32,
    pub(super) mode: u32,
    pub(super) size: u64,
    pub(super) modified_seconds: i64,
    pub(super) modified_nanoseconds: i64,
    pub(super) changed_seconds: i64,
    pub(super) changed_nanoseconds: i64,
}

pub(super) fn read_request_file(
    inbox: &File,
    user_id: u32,
    group_id: u32,
) -> Result<(Vec<u8>, RequestMetadata), ProbeError> {
    let (bytes, before, after) = read_once(inbox, user_id, group_id)?;
    let (confirmed, reopened, final_state) = read_once(inbox, user_id, group_id)?;
    ensure_stable_identity(before, after, reopened, final_state)?;
    ensure_stable_content(&bytes, &confirmed)?;
    Ok((bytes, final_state))
}

pub(super) fn ensure_stable_identity(
    before: RequestMetadata,
    after: RequestMetadata,
    reopened: RequestMetadata,
    final_state: RequestMetadata,
) -> Result<(), ProbeError> {
    if before == after && after == reopened && reopened == final_state {
        Ok(())
    } else {
        Err(ProbeError::Invalid("execution_request_changed"))
    }
}

pub(super) fn ensure_stable_content(first: &[u8], second: &[u8]) -> Result<(), ProbeError> {
    if first == second {
        Ok(())
    } else {
        Err(ProbeError::Invalid("execution_request_changed"))
    }
}

fn read_once(
    inbox: &File,
    user_id: u32,
    group_id: u32,
) -> Result<(Vec<u8>, RequestMetadata, RequestMetadata), ProbeError> {
    let file = open_request(inbox)?;
    let before = request_metadata(&file, user_id, group_id)?;
    let mut bytes = Vec::with_capacity(REQUEST_LIMIT.min(4096));
    file.try_clone()
        .map_err(|error| ProbeError::Read("execution_request_clone", error))?
        .take((REQUEST_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read("execution_request_read", error))?;
    if bytes.len() > REQUEST_LIMIT {
        return Err(ProbeError::InputTooLarge("execution_request"));
    }
    let after = request_metadata(&file, user_id, group_id)?;
    Ok((bytes, before, after))
}

fn open_request(inbox: &File) -> Result<File, ProbeError> {
    openat(
        inbox,
        REQUEST_FILE,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|_| ProbeError::Invalid("execution_request_open"))
}

fn request_metadata(
    file: &File,
    user_id: u32,
    group_id: u32,
) -> Result<RequestMetadata, ProbeError> {
    let metadata = file
        .metadata()
        .map_err(|error| ProbeError::Read("execution_request_stat", error))?;
    let value = RequestMetadata {
        inode: metadata.ino(),
        user_id: metadata.uid(),
        group_id: metadata.gid(),
        mode: metadata.mode() & 0o777,
        size: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
    };
    if !metadata.is_file()
        || value.user_id != user_id
        || value.group_id != group_id
        || value.mode != 0o600
        || value.size > REQUEST_LIMIT as u64
    {
        return Err(ProbeError::Invalid("execution_request_file"));
    }
    Ok(value)
}

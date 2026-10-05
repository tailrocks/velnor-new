//! Bounded raw TAR admission before the `tar` crate can consume extension data.

use std::io::Read;

use tar::EntryType;

use crate::OrchestratorError;
use crate::internal::internal;

use super::{MAX_ENTRIES, MAX_ENTRY_BYTES, MAX_TOTAL_BYTES, archive_error};

const TAR_BLOCK_BYTES: u64 = 512;
pub(super) const MAX_TAR_EXTENSION_ENTRY_BYTES: u64 = 64 * 1024;
const MAX_TAR_EXTENSION_BYTES: u64 = 1024 * 1024;
const MAX_TAR_EXTENSION_ENTRIES: usize = 64;

pub(super) fn preflight_tar<R: Read>(mut reader: R) -> Result<usize, OrchestratorError> {
    let mut count = 0_usize;
    let mut total = 0_u64;
    let mut extension_count = 0_usize;
    let mut extension_bytes = 0_u64;
    loop {
        let mut header = tar::Header::new_old();
        if !read_tar_header(&mut reader, &mut header)? {
            break;
        }
        if header.as_bytes().iter().all(|byte| *byte == 0) {
            break;
        }
        count = count
            .checked_add(1)
            .ok_or_else(|| internal("tool_archive_entry_limit"))?;
        if count > MAX_ENTRIES {
            return Err(internal("tool_archive_entry_limit"));
        }
        let size = header
            .entry_size()
            .map_err(|error| archive_error(&error.to_string()))?;
        if size > MAX_ENTRY_BYTES {
            return Err(internal("tool_archive_entry_size_limit"));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| internal("tool_archive_total_size_limit"))?;
        if total > MAX_TOTAL_BYTES {
            return Err(internal("tool_archive_total_size_limit"));
        }
        if is_tar_extension(header.entry_type()) {
            extension_count = extension_count
                .checked_add(1)
                .ok_or_else(|| internal("tool_archive_metadata_entry_limit"))?;
            if extension_count > MAX_TAR_EXTENSION_ENTRIES || size > MAX_TAR_EXTENSION_ENTRY_BYTES {
                return Err(internal("tool_archive_metadata_entry_size_limit"));
            }
            extension_bytes = extension_bytes
                .checked_add(size)
                .ok_or_else(|| internal("tool_archive_metadata_size_limit"))?;
            if extension_bytes > MAX_TAR_EXTENSION_BYTES {
                return Err(internal("tool_archive_metadata_size_limit"));
            }
        }
        skip_tar_entry(&mut reader, size)?;
    }
    Ok(extension_count)
}

fn read_tar_header<R: Read>(
    reader: &mut R,
    header: &mut tar::Header,
) -> Result<bool, OrchestratorError> {
    let first = reader
        .read(&mut header.as_mut_bytes()[..1])
        .map_err(|error| archive_error(&error.to_string()))?;
    if first == 0 {
        return Ok(false);
    }
    reader
        .read_exact(&mut header.as_mut_bytes()[1..])
        .map_err(|error| archive_error(&error.to_string()))?;
    Ok(true)
}

fn skip_tar_entry<R: Read>(reader: &mut R, size: u64) -> Result<(), OrchestratorError> {
    let padding = (TAR_BLOCK_BYTES - size % TAR_BLOCK_BYTES) % TAR_BLOCK_BYTES;
    let mut remaining = size
        .checked_add(padding)
        .ok_or_else(|| internal("tool_archive_total_size_limit"))?;
    let mut buffer = [0_u8; 8192];
    while remaining > 0 {
        let read = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| internal("tool_archive_total_size_limit"))?;
        let count = reader
            .read(&mut buffer[..read])
            .map_err(|error| archive_error(&error.to_string()))?;
        if count == 0 {
            return Err(internal("tool_archive_truncated"));
        }
        remaining -= count as u64;
    }
    Ok(())
}

fn is_tar_extension(kind: EntryType) -> bool {
    kind.is_gnu_longname()
        || kind.is_gnu_longlink()
        || kind.is_pax_local_extensions()
        || kind.is_pax_global_extensions()
}

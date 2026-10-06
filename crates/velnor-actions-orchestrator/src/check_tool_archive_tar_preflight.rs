//! Bounded raw TAR admission before the `tar` crate can consume extension data.

use std::io::Read;

use tar::EntryType;

use crate::OrchestratorError;
use crate::internal::internal;

use super::{MAX_ENTRIES, MAX_ENTRY_BYTES, MAX_TOTAL_BYTES, archive_error};

const TAR_BLOCK_BYTES: u64 = 512;
pub(super) const MAX_TAR_EXTENSION_ENTRY_BYTES: u64 = 64 * 1024;
const MAX_TAR_EXTENSION_BYTES: u64 = 1024 * 1024;
// The pinned Rust 1.98.1 Linux std archive contains 65 GNU long-name records;
// 256 caps extension headers at 128 KiB while payloads remain capped at 1 MiB.
pub(super) const MAX_TAR_EXTENSION_ENTRIES: usize = 256;

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
        let kind = header.entry_type();
        if kind.is_gnu_sparse() {
            return Err(internal("tool_archive_sparse_unsupported"));
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
        if is_tar_extension(kind) {
            extension_count = extension_count
                .checked_add(1)
                .ok_or_else(|| internal("tool_archive_metadata_entry_limit"))?;
            if extension_count > MAX_TAR_EXTENSION_ENTRIES {
                return Err(internal("tool_archive_metadata_entry_limit"));
            }
            if size > MAX_TAR_EXTENSION_ENTRY_BYTES {
                return Err(internal("tool_archive_metadata_entry_size_limit"));
            }
            extension_bytes = extension_bytes
                .checked_add(size)
                .ok_or_else(|| internal("tool_archive_metadata_size_limit"))?;
            if extension_bytes > MAX_TAR_EXTENSION_BYTES {
                return Err(internal("tool_archive_metadata_size_limit"));
            }
        }
        if kind.is_pax_local_extensions() || kind.is_pax_global_extensions() {
            let metadata = read_tar_extension(&mut reader, size)?;
            reject_framing_pax(&metadata)?;
            skip_tar_padding(&mut reader, size)?;
        } else {
            skip_tar_entry(&mut reader, size)?;
        }
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
    skip_tar_data(reader, size)?;
    skip_tar_padding(reader, size)
}

fn read_tar_extension<R: Read>(reader: &mut R, size: u64) -> Result<Vec<u8>, OrchestratorError> {
    let length = usize::try_from(size).map_err(|_| internal("tool_archive_metadata_size_limit"))?;
    let mut payload = vec![0_u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|error| archive_error(&error.to_string()))?;
    Ok(payload)
}

fn skip_tar_data<R: Read>(reader: &mut R, size: u64) -> Result<(), OrchestratorError> {
    skip_tar_bytes(reader, size)
}

fn skip_tar_padding<R: Read>(reader: &mut R, size: u64) -> Result<(), OrchestratorError> {
    let padding = (TAR_BLOCK_BYTES - size % TAR_BLOCK_BYTES) % TAR_BLOCK_BYTES;
    skip_tar_bytes(reader, padding)
}

fn skip_tar_bytes<R: Read>(reader: &mut R, size: u64) -> Result<(), OrchestratorError> {
    let mut remaining = size;
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

fn reject_framing_pax(payload: &[u8]) -> Result<(), OrchestratorError> {
    let mut remaining = payload;
    while !remaining.is_empty() {
        let line_end = remaining
            .iter()
            .position(|byte| *byte == b'\n')
            .unwrap_or(remaining.len());
        let line = &remaining[..line_end];
        remaining = if line_end == remaining.len() {
            &[]
        } else {
            &remaining[line_end + 1..]
        };
        if line.is_empty() {
            if remaining.is_empty() {
                break;
            }
            return Err(internal("tool_archive_pax_record_invalid"));
        }
        let separator = line
            .iter()
            .position(|byte| *byte == b' ')
            .ok_or_else(|| internal("tool_archive_pax_record_invalid"))?;
        let length = std::str::from_utf8(&line[..separator])
            .ok()
            .and_then(|digits| digits.parse::<usize>().ok())
            .ok_or_else(|| internal("tool_archive_pax_record_invalid"))?;
        if length != line.len().saturating_add(1) || line.len() <= separator + 1 {
            return Err(internal("tool_archive_pax_record_invalid"));
        }
        let fields = &line[separator + 1..];
        let equals = fields
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or_else(|| internal("tool_archive_pax_record_invalid"))?;
        if equals == 0 {
            return Err(internal("tool_archive_pax_record_invalid"));
        }
        let key = &fields[..equals];
        if key == b"size" {
            return Err(internal("tool_archive_pax_size_override"));
        }
        if key == b"GNU.sparse" || key.starts_with(b"GNU.sparse.") {
            return Err(internal("tool_archive_sparse_unsupported"));
        }
    }
    Ok(())
}

fn is_tar_extension(kind: EntryType) -> bool {
    kind.is_gnu_longname()
        || kind.is_gnu_longlink()
        || kind.is_pax_local_extensions()
        || kind.is_pax_global_extensions()
}

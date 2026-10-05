//! ZIP directory admission and special-entry rejection.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::OrchestratorError;
use crate::internal::internal;
use velnor_actions_mise::CheckDeadline;

use super::{MAX_ENTRIES, check_deadline, io_error};

pub(super) fn preflight_zip_entries(
    source: &mut File,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    const EOCD_BYTES: u64 = 22 + 65_535 + 20 + 56;
    let length = source
        .metadata()
        .map_err(|error| io_error(Path::new("zip"), error))?
        .len();
    let start = length.saturating_sub(EOCD_BYTES);
    source
        .seek(SeekFrom::Start(start))
        .map_err(|error| io_error(Path::new("zip"), error))?;
    let mut tail = Vec::new();
    source
        .take(EOCD_BYTES)
        .read_to_end(&mut tail)
        .map_err(|error| io_error(Path::new("zip"), error))?;
    check_deadline(deadline)?;
    let marker = b"PK\x05\x06";
    let eocd = tail
        .windows(marker.len())
        .rposition(|window| window == marker)
        .ok_or_else(|| internal("tool_archive_zip_eocd"))?;
    if tail.len().saturating_sub(eocd) < 22 {
        return Err(internal("tool_archive_zip_eocd"));
    }
    let count = u16::from_le_bytes([tail[eocd + 10], tail[eocd + 11]]);
    let count = if count == u16::MAX {
        zip64_entry_count(&tail, start, eocd)?
    } else {
        u64::from(count)
    };
    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error(Path::new("zip"), error))?;
    if count > MAX_ENTRIES as u64 {
        return Err(internal("tool_archive_entry_limit"));
    }
    check_deadline(deadline)
}

fn zip64_entry_count(tail: &[u8], start: u64, eocd: usize) -> Result<u64, OrchestratorError> {
    let marker = b"PK\x06\x07";
    let locator = tail[..eocd]
        .windows(marker.len())
        .rposition(|window| window == marker)
        .ok_or_else(|| internal("tool_archive_zip64_entries"))?;
    if locator.saturating_add(20) > eocd {
        return Err(internal("tool_archive_zip64_entries"));
    }
    let absolute = u64::from_le_bytes(
        tail[locator + 8..locator + 16]
            .try_into()
            .map_err(|_| internal("tool_archive_zip64_entries"))?,
    );
    let relative = absolute
        .checked_sub(start)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| internal("tool_archive_zip64_entries"))?;
    if relative.saturating_add(40) > tail.len() || &tail[relative..relative + 4] != b"PK\x06\x06" {
        return Err(internal("tool_archive_zip64_entries"));
    }
    Ok(u64::from_le_bytes(
        tail[relative + 32..relative + 40]
            .try_into()
            .map_err(|_| internal("tool_archive_zip64_entries"))?,
    ))
}

pub(super) fn reject_zip_special(mode: u32, directory: bool) -> Result<(), OrchestratorError> {
    let kind = mode & 0o170_000;
    let expected = if directory { 0o040_000 } else { 0o100_000 };
    if kind != 0 && kind != expected {
        return Err(internal("tool_archive_special_entry"));
    }
    Ok(())
}

//! ZIP directory admission and special-entry rejection.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use velnor_actions_mise::CheckDeadline;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal;

use super::{MAX_ENTRIES, check_deadline, io_error};

const SCAN_BYTES: usize = 64 * 1024;
const ZIP64_RECORD_MAX_BYTES: u64 = 44 + 64 * 1024;
const ZIP_MARKER_LIMIT: usize = MAX_ENTRIES + 1;

struct ZipMarkers {
    ends: Vec<u64>,
    central: Vec<u64>,
    zip64_ends: Vec<u64>,
}

pub(super) fn preflight_zip_entries(
    source: &mut File,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    let length = source
        .metadata()
        .map_err(|error| io_error(Path::new("zip"), error))?
        .len();
    let markers = scan_markers(source, length, deadline)?;
    admit_possible_directories(source, length, &markers, deadline)?;
    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error(Path::new("zip"), error))?;
    check_deadline(deadline)
}

fn scan_markers(
    source: &mut File,
    length: u64,
    deadline: CheckDeadline,
) -> Result<ZipMarkers, OrchestratorError> {
    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error(Path::new("zip"), error))?;
    let mut bytes = vec![0_u8; SCAN_BYTES + 3].into_boxed_slice();
    let mut carry = 0_usize;
    let mut consumed = 0_u64;
    let mut markers = ZipMarkers {
        ends: Vec::new(),
        central: Vec::new(),
        zip64_ends: Vec::new(),
    };
    loop {
        check_deadline(deadline)?;
        let count = source
            .read(&mut bytes[carry..])
            .map_err(|error| io_error(Path::new("zip"), error))?;
        if count == 0 {
            break;
        }
        let used = carry + count;
        let window_start = consumed.saturating_sub(carry as u64);
        for (index, window) in bytes[..used].windows(4).enumerate() {
            let position = window_start + index as u64;
            match window {
                b"PK\x05\x06" => push_marker(&mut markers.ends, position)?,
                b"PK\x01\x02" => push_marker(&mut markers.central, position)?,
                b"PK\x06\x06" => push_marker(&mut markers.zip64_ends, position)?,
                _ => {}
            }
        }
        consumed = consumed.saturating_add(count as u64);
        carry = used.min(3);
        bytes.copy_within(used - carry..used, 0);
    }
    if consumed != length {
        return Err(internal("tool_archive_changed_during_preflight"));
    }
    check_deadline(deadline)?;
    Ok(markers)
}

fn push_marker(markers: &mut Vec<u64>, position: u64) -> Result<(), OrchestratorError> {
    if markers.len() >= ZIP_MARKER_LIMIT {
        return Err(internal("tool_archive_zip_marker_limit"));
    }
    markers.push(position);
    Ok(())
}

fn admit_possible_directories(
    source: &mut File,
    length: u64,
    markers: &ZipMarkers,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for eocd in markers.ends.iter().rev().copied() {
        check_deadline(deadline)?;
        if eocd.checked_add(22).is_none_or(|end| end > length) {
            continue;
        }
        let end = read_exact_at::<22>(source, eocd, deadline)?;
        let comment_end = eocd
            .checked_add(22)
            .and_then(|position| position.checked_add(u64::from(le_u16(&end[20..22]))));
        if comment_end.is_none_or(|position| position > length) {
            continue;
        }

        let zip64 = le_u16(&end[10..12]) == u16::MAX
            || le_u32(&end[12..16]) == u32::MAX
            || le_u32(&end[16..20]) == u32::MAX;
        if zip64 && try_admit_zip64_directory(source, eocd, end, markers, deadline)? {
            // A valid ZIP64 locator commits the pinned reader to its ZIP64 path,
            // including when no matching end record is found. It does not fall
            // back to the ZIP32 fields for this candidate.
            continue;
        }
        if !zip32_directory_can_be_selected(eocd, end, &markers.central) {
            continue;
        }
        let disk = le_u16(&end[4..6]);
        let directory_disk = le_u16(&end[6..8]);
        let entries_on_disk = le_u16(&end[8..10]);
        let entries_total = le_u16(&end[10..12]);
        if disk != 0 || directory_disk != 0 {
            return Err(internal("tool_archive_zip_multidisk"));
        }
        if entries_on_disk != entries_total {
            return Err(internal("tool_archive_zip_entry_count_mismatch"));
        }
        if u64::from(entries_on_disk) > MAX_ENTRIES as u64 {
            return Err(internal("tool_archive_entry_limit"));
        }
    }
    Ok(())
}

fn zip32_directory_can_be_selected(eocd: u64, end: [u8; 22], central: &[u64]) -> bool {
    if le_u16(&end[10..12]) == 0 {
        return true;
    }
    let start = u64::from(le_u32(&end[16..20]));
    if start >= eocd {
        return false;
    }
    let candidate = central.partition_point(|position| *position < start);
    central
        .get(candidate)
        .is_some_and(|position| *position < eocd)
}

fn try_admit_zip64_directory(
    source: &mut File,
    eocd: u64,
    zip32_end: [u8; 22],
    markers: &ZipMarkers,
    deadline: CheckDeadline,
) -> Result<bool, OrchestratorError> {
    let locator_offset = eocd
        .checked_sub(20)
        .ok_or_else(|| internal("tool_archive_zip64_locator"))?;
    let locator = read_exact_at::<20>(source, locator_offset, deadline)?;
    if &locator[..4] != b"PK\x06\x07" {
        return Ok(false);
    }
    let locator_disk = le_u32(&locator[4..8]);
    let record_search_start = le_u64(&locator[8..16]);
    let disk_count = le_u32(&locator[16..20]);
    if locator_disk != 0 || disk_count != 1 {
        return Err(internal("tool_archive_zip_multidisk"));
    }
    if record_search_start >= locator_offset {
        return Ok(true);
    }
    for record_offset in markers
        .zip64_ends
        .iter()
        .copied()
        .filter(|position| *position >= record_search_start && *position < locator_offset)
    {
        check_deadline(deadline)?;
        let expected_bytes = locator_offset - record_offset;
        if expected_bytes < 56 {
            continue;
        }
        let record = read_exact_at::<56>(source, record_offset, deadline)?;
        let record_size = le_u64(&record[4..12]);
        if record_size < 40
            || record_size
                .checked_add(12)
                .is_none_or(|size| size > expected_bytes)
        {
            continue;
        }
        if record_size > ZIP64_RECORD_MAX_BYTES {
            return Err(internal("tool_archive_zip64_record_size"));
        }
        if record_size.checked_add(12) != Some(expected_bytes) {
            continue;
        }

        let directory_disk = le_u32(&record[20..24]);
        let entries_on_disk = le_u64(&record[24..32]);
        let entries_total = le_u64(&record[32..40]);
        let directory_offset = le_u64(&record[48..56]);
        if directory_disk != locator_disk || entries_on_disk > entries_total {
            continue;
        }
        let minimum_end = entries_total
            .saturating_mul(46)
            .saturating_add(directory_offset);
        if record_offset < minimum_end {
            continue;
        }

        if entries_total > MAX_ENTRIES as u64 {
            return Err(internal("tool_archive_entry_limit"));
        }
        let disk = le_u32(&record[16..20]);
        if disk != 0 || directory_disk != 0 {
            return Err(internal("tool_archive_zip_multidisk"));
        }
        if entries_on_disk != entries_total {
            return Err(internal("tool_archive_zip_entry_count_mismatch"));
        }
        let zip32_disk_count = le_u16(&zip32_end[8..10]);
        let zip32_total_count = le_u16(&zip32_end[10..12]);
        if zip32_disk_count != u16::MAX && u64::from(zip32_disk_count) != entries_on_disk {
            return Err(internal("tool_archive_zip_entry_count_mismatch"));
        }
        if zip32_total_count != u16::MAX && u64::from(zip32_total_count) != entries_total {
            return Err(internal("tool_archive_zip_entry_count_mismatch"));
        }
    }
    check_deadline(deadline)?;
    Ok(true)
}

fn read_exact_at<const N: usize>(
    source: &mut File,
    offset: u64,
    deadline: CheckDeadline,
) -> Result<[u8; N], OrchestratorError> {
    check_deadline(deadline)?;
    source
        .seek(SeekFrom::Start(offset))
        .map_err(|error| io_error(Path::new("zip"), error))?;
    let mut bytes = [0_u8; N];
    source
        .read_exact(&mut bytes)
        .map_err(|error| io_error(Path::new("zip"), error))?;
    check_deadline(deadline)?;
    Ok(bytes)
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

pub(super) fn reject_zip_special(mode: u32, directory: bool) -> Result<(), OrchestratorError> {
    let kind = mode & 0o170_000;
    let expected = if directory { 0o040_000 } else { 0o100_000 };
    if kind != 0 && kind != expected {
        return Err(internal("tool_archive_special_entry"));
    }
    Ok(())
}

use std::io::{Read, Seek, SeekFrom};

const EOCD_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
const ZIP64_LOCATOR_SIGNATURE: &[u8; 4] = b"PK\x06\x07";
const ZIP64_EOCD_SIGNATURE: &[u8; 4] = b"PK\x06\x06";
const EOCD_FIXED_BYTES: usize = 22;
const MAX_EOCD_COMMENT_BYTES: usize = u16::MAX as usize;

/// Read the archive's declared central-directory entry count without allowing
/// `ZipArchive`'s filename map to hide duplicate raw names.
pub(super) fn central_entry_count<R: Read + Seek>(
    reader: &mut R,
    archive_bytes: u64,
) -> Result<u64, &'static str> {
    let file_len = reader
        .seek(SeekFrom::End(0))
        .map_err(|_| "artifact_archive_invalid")?;
    if file_len != archive_bytes {
        return Err("artifact_archive_size_mismatch");
    }
    let tail_bytes = file_len.min(
        u64::try_from(EOCD_FIXED_BYTES + MAX_EOCD_COMMENT_BYTES)
            .map_err(|_| "artifact_archive_invalid")?,
    );
    let tail_start = file_len
        .checked_sub(tail_bytes)
        .ok_or("artifact_archive_invalid")?;
    reader
        .seek(SeekFrom::Start(tail_start))
        .map_err(|_| "artifact_archive_invalid")?;
    let mut tail = vec![0; usize::try_from(tail_bytes).map_err(|_| "artifact_archive_invalid")?];
    reader
        .read_exact(&mut tail)
        .map_err(|_| "artifact_archive_invalid")?;

    let eocd = (0..=tail.len().saturating_sub(EOCD_FIXED_BYTES))
        .rev()
        .find(|offset| {
            tail.get(*offset..offset.saturating_add(4)) == Some(EOCD_SIGNATURE.as_slice())
                && read_u16(&tail, offset + 20).is_some_and(|comment_bytes| {
                    offset
                        .checked_add(EOCD_FIXED_BYTES)
                        .and_then(|start| start.checked_add(usize::from(comment_bytes)))
                        == Some(tail.len())
                })
        })
        .ok_or("artifact_archive_invalid")?;
    let disk_number = read_u16(&tail, eocd + 4).ok_or("artifact_archive_invalid")?;
    let central_disk = read_u16(&tail, eocd + 6).ok_or("artifact_archive_invalid")?;
    let entries_on_disk = read_u16(&tail, eocd + 8).ok_or("artifact_archive_invalid")?;
    let total_entries = read_u16(&tail, eocd + 10).ok_or("artifact_archive_invalid")?;
    if disk_number != 0 || central_disk != 0 {
        return Err("artifact_archive_invalid");
    }
    if entries_on_disk != u16::MAX && total_entries != u16::MAX {
        if entries_on_disk != total_entries {
            return Err("artifact_archive_invalid");
        }
        return Ok(u64::from(total_entries));
    }

    let eocd_absolute = tail_start
        .checked_add(u64::try_from(eocd).map_err(|_| "artifact_archive_invalid")?)
        .ok_or("artifact_archive_invalid")?;
    zip64_total_entry_count(reader, eocd_absolute)
}

fn zip64_total_entry_count<R: Read + Seek>(
    reader: &mut R,
    eocd_absolute: u64,
) -> Result<u64, &'static str> {
    let locator_absolute = eocd_absolute
        .checked_sub(20)
        .ok_or("artifact_archive_zip64_invalid")?;
    let mut locator = [0_u8; 20];
    reader
        .seek(SeekFrom::Start(locator_absolute))
        .and_then(|_| reader.read_exact(&mut locator))
        .map_err(|_| "artifact_archive_zip64_invalid")?;
    if locator.get(..4) != Some(ZIP64_LOCATOR_SIGNATURE.as_slice())
        || read_u32(&locator, 4) != Some(0)
        || read_u32(&locator, 16) != Some(1)
    {
        return Err("artifact_archive_zip64_invalid");
    }
    let zip64_eocd_absolute = read_u64(&locator, 8).ok_or("artifact_archive_zip64_invalid")?;
    let mut zip64_eocd = [0_u8; 56];
    reader
        .seek(SeekFrom::Start(zip64_eocd_absolute))
        .and_then(|_| reader.read_exact(&mut zip64_eocd))
        .map_err(|_| "artifact_archive_zip64_invalid")?;
    if zip64_eocd.get(..4) != Some(ZIP64_EOCD_SIGNATURE.as_slice())
        || read_u64(&zip64_eocd, 4).is_none_or(|record_bytes| record_bytes < 44)
        || read_u32(&zip64_eocd, 16) != Some(0)
        || read_u32(&zip64_eocd, 20) != Some(0)
    {
        return Err("artifact_archive_zip64_invalid");
    }
    let entries_on_disk = read_u64(&zip64_eocd, 24).ok_or("artifact_archive_zip64_invalid")?;
    let total_entries = read_u64(&zip64_eocd, 32).ok_or("artifact_archive_zip64_invalid")?;
    if entries_on_disk != total_entries {
        return Err("artifact_archive_zip64_invalid");
    }
    Ok(total_entries)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
#[path = "zip_directory/tests.rs"]
mod tests;

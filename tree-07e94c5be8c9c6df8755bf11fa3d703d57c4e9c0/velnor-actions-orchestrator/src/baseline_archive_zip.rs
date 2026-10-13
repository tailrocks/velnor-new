//! Raw ZIP structure checks before the ZIP crate decodes one entry.

use super::MAX_BASELINE_ARCHIVE_BYTES;

#[path = "baseline_archive_deflate.rs"]
mod deflate;

const MAX_MANIFEST_BYTES: u64 = 1_048_576;
const EOCD_BYTES: usize = 22;
const CENTRAL_BYTES: usize = 46;
const LOCAL_BYTES: usize = 30;
const DATA_DESCRIPTOR_SIGNATURE: u32 = 0x0807_4b50;

pub(super) struct ZipShape {
    pub(super) compressed_size: u32,
    pub(super) uncompressed_size: u32,
    pub(super) method: u16,
}

/// Check that a raw DEFLATE stream ends at the exact declared boundary.
pub(super) fn exact_deflate_consumption(compressed: &[u8], size: u32) -> bool {
    deflate::exact_consumption(compressed, size)
}

/// Reject Zip64, multidisk, encryption, and local/central disagreement first.
pub(super) fn preflight_zip(bytes: &[u8]) -> Result<ZipShape, String> {
    if bytes.len() < EOCD_BYTES || bytes.len() > MAX_BASELINE_ARCHIVE_BYTES {
        return Err("baseline_archive_shape".to_owned());
    }
    let eocd = find_eocd(bytes).ok_or_else(invalid_archive)?;
    let disk = read_u16(bytes, eocd + 4).ok_or_else(invalid_archive)?;
    let central_disk = read_u16(bytes, eocd + 6).ok_or_else(invalid_archive)?;
    let disk_entries = read_u16(bytes, eocd + 8).ok_or_else(invalid_archive)?;
    let total_entries = read_u16(bytes, eocd + 10).ok_or_else(invalid_archive)?;
    let central_size = read_u32(bytes, eocd + 12).ok_or_else(invalid_archive)?;
    let central_offset = read_u32(bytes, eocd + 16).ok_or_else(invalid_archive)?;
    if disk != 0
        || central_disk != 0
        || disk_entries != 1
        || total_entries != 1
        || central_size == u32::MAX
        || central_offset == u32::MAX
        || has_zip64_locator(bytes, eocd)
        || usize::try_from(central_offset).ok().and_then(|offset| {
            usize::try_from(central_size)
                .ok()
                .and_then(|size| offset.checked_add(size))
        }) != Some(eocd)
    {
        return Err("baseline_archive_shape".to_owned());
    }
    central_entry(bytes, central_offset, central_size, eocd)
}

/// Validate the sole central record and its matching local record.
fn central_entry(
    bytes: &[u8],
    central_offset: u32,
    central_size: u32,
    eocd: usize,
) -> Result<ZipShape, String> {
    let central = usize::try_from(central_offset).map_err(|_| invalid_archive())?;
    if read_u32(bytes, central) != Some(0x0201_4b50) {
        return Err("baseline_archive_invalid".to_owned());
    }
    let fields = central_fields(bytes, central)?;
    let end = central
        .checked_add(CENTRAL_BYTES)
        .and_then(|offset| offset.checked_add(fields.name_len))
        .and_then(|offset| offset.checked_add(fields.extra_len))
        .and_then(|offset| offset.checked_add(fields.comment_len))
        .ok_or_else(invalid_archive)?;
    if end != eocd
        || usize::try_from(central_size).ok() != end.checked_sub(central)
        || fields.disk_start != 0
        || fields.local_offset != 0
        || fields.name != b"baseline.json"
        || fields.compressed_size == u32::MAX
        || fields.uncompressed_size == u32::MAX
        || u64::from(fields.uncompressed_size) > MAX_MANIFEST_BYTES
        || fields.compressed_size as usize > MAX_BASELINE_ARCHIVE_BYTES
        || !(10..=20).contains(&fields.version_needed)
        || !regular_file(fields.system, fields.external_attributes)
        || !safe_extra(bytes, fields.extra_offset, fields.extra_len)
        || !safe_flags(fields.flags, fields.method)
    {
        return Err("baseline_archive_shape".to_owned());
    }
    local_entry(bytes, central, &fields)
}

struct CentralFields<'a> {
    name: &'a [u8],
    name_len: usize,
    extra_offset: usize,
    extra_len: usize,
    comment_len: usize,
    disk_start: u16,
    local_offset: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    flags: u16,
    method: u16,
    crc32: u32,
    external_attributes: u32,
    system: u8,
    version_needed: u16,
    mod_time: u16,
    mod_date: u16,
}

/// Read the fixed central header and its bounded name/extra fields.
fn central_fields(bytes: &[u8], offset: usize) -> Result<CentralFields<'_>, String> {
    let system_version = read_u16(bytes, offset + 4).ok_or_else(invalid_archive)?;
    let version_needed = read_u16(bytes, offset + 6).ok_or_else(invalid_archive)?;
    let flags = read_u16(bytes, offset + 8).ok_or_else(invalid_archive)?;
    let method = read_u16(bytes, offset + 10).ok_or_else(invalid_archive)?;
    let crc32 = read_u32(bytes, offset + 16).ok_or_else(invalid_archive)?;
    let compressed_size = read_u32(bytes, offset + 20).ok_or_else(invalid_archive)?;
    let uncompressed_size = read_u32(bytes, offset + 24).ok_or_else(invalid_archive)?;
    let name_len = usize::from(read_u16(bytes, offset + 28).ok_or_else(invalid_archive)?);
    let extra_len = usize::from(read_u16(bytes, offset + 30).ok_or_else(invalid_archive)?);
    let comment_len = usize::from(read_u16(bytes, offset + 32).ok_or_else(invalid_archive)?);
    let disk_start = read_u16(bytes, offset + 34).ok_or_else(invalid_archive)?;
    let external_attributes = read_u32(bytes, offset + 38).ok_or_else(invalid_archive)?;
    let local_offset = read_u32(bytes, offset + 42).ok_or_else(invalid_archive)?;
    let name_offset = offset
        .checked_add(CENTRAL_BYTES)
        .ok_or_else(invalid_archive)?;
    let extra_offset = name_offset
        .checked_add(name_len)
        .ok_or_else(invalid_archive)?;
    let name_end = name_offset
        .checked_add(name_len)
        .ok_or_else(invalid_archive)?;
    let end = extra_offset
        .checked_add(extra_len)
        .and_then(|value| value.checked_add(comment_len))
        .ok_or_else(invalid_archive)?;
    let name = bytes
        .get(name_offset..name_end)
        .filter(|_| end <= bytes.len())
        .ok_or_else(invalid_archive)?;
    Ok(CentralFields {
        name,
        name_len,
        extra_offset,
        extra_len,
        comment_len,
        disk_start,
        local_offset,
        compressed_size,
        uncompressed_size,
        flags,
        method,
        crc32,
        external_attributes,
        system: (system_version >> 8) as u8,
        version_needed,
        mod_time: read_u16(bytes, offset + 12).ok_or_else(invalid_archive)?,
        mod_date: read_u16(bytes, offset + 14).ok_or_else(invalid_archive)?,
    })
}

/// Require central metadata and the local record to agree byte-for-byte.
fn local_entry(
    bytes: &[u8],
    central: usize,
    fields: &CentralFields<'_>,
) -> Result<ZipShape, String> {
    if read_u32(bytes, 0) != Some(0x0403_4b50) {
        return Err("baseline_archive_invalid".to_owned());
    }
    let version = read_u16(bytes, 4).ok_or_else(invalid_archive)?;
    let flags = read_u16(bytes, 6).ok_or_else(invalid_archive)?;
    let method = read_u16(bytes, 8).ok_or_else(invalid_archive)?;
    let time = read_u16(bytes, 10).ok_or_else(invalid_archive)?;
    let date = read_u16(bytes, 12).ok_or_else(invalid_archive)?;
    let crc = read_u32(bytes, 14).ok_or_else(invalid_archive)?;
    let compressed = read_u32(bytes, 18).ok_or_else(invalid_archive)?;
    let uncompressed = read_u32(bytes, 22).ok_or_else(invalid_archive)?;
    let name_len = usize::from(read_u16(bytes, 26).ok_or_else(invalid_archive)?);
    let extra_len = usize::from(read_u16(bytes, 28).ok_or_else(invalid_archive)?);
    let name_end = LOCAL_BYTES
        .checked_add(name_len)
        .ok_or_else(invalid_archive)?;
    let extra_end = name_end
        .checked_add(extra_len)
        .ok_or_else(invalid_archive)?;
    let data_end = extra_end
        .checked_add(usize::try_from(fields.compressed_size).map_err(|_| invalid_archive())?)
        .ok_or_else(invalid_archive)?;
    let local_name = bytes
        .get(LOCAL_BYTES..name_end)
        .ok_or_else(invalid_archive)?;
    let local_extra = bytes.get(name_end..extra_end).ok_or_else(invalid_archive)?;
    let has_descriptor = fields.flags & 0x0008 != 0;
    let local_sizes_match = if has_descriptor {
        crc == 0 && compressed == 0 && uncompressed == 0
    } else {
        crc == fields.crc32
            && compressed == fields.compressed_size
            && uncompressed == fields.uncompressed_size
    };
    let following_record_matches = if has_descriptor {
        data_descriptor_matches(bytes, data_end, central, fields)
    } else {
        data_end == central
    };
    if extra_end > bytes.len()
        || !following_record_matches
        || local_name != fields.name
        || !safe_extra(local_extra, 0, local_extra.len())
        || version != fields.version_needed
        || flags != fields.flags
        || method != fields.method
        || time != fields.mod_time
        || date != fields.mod_date
        || !local_sizes_match
    {
        return Err("baseline_archive_local_central_mismatch".to_owned());
    }
    Ok(ZipShape {
        compressed_size: fields.compressed_size,
        uncompressed_size: fields.uncompressed_size,
        method: fields.method,
    })
}

/// Match the optional-signature 32-bit descriptor to the central record.
fn data_descriptor_matches(
    bytes: &[u8],
    offset: usize,
    central: usize,
    fields: &CentralFields<'_>,
) -> bool {
    let Some(length) = central.checked_sub(offset) else {
        return false;
    };
    match length {
        12 => {
            read_u32(bytes, offset) == Some(fields.crc32)
                && read_u32(bytes, offset + 4) == Some(fields.compressed_size)
                && read_u32(bytes, offset + 8) == Some(fields.uncompressed_size)
        }
        16 => {
            read_u32(bytes, offset) == Some(DATA_DESCRIPTOR_SIGNATURE)
                && read_u32(bytes, offset + 4) == Some(fields.crc32)
                && read_u32(bytes, offset + 8) == Some(fields.compressed_size)
                && read_u32(bytes, offset + 12) == Some(fields.uncompressed_size)
        }
        _ => false,
    }
}

/// Find the unique EOCD whose declared comment ends exactly at archive end.
fn find_eocd(bytes: &[u8]) -> Option<usize> {
    let start = bytes
        .len()
        .saturating_sub(EOCD_BYTES + usize::from(u16::MAX));
    let last = bytes.len().checked_sub(EOCD_BYTES)?;
    let mut found = None;
    for offset in (start..=last).rev() {
        if read_u32(bytes, offset) != Some(0x0605_4b50) {
            continue;
        }
        let comment = usize::from(read_u16(bytes, offset + 20)?);
        if offset.checked_add(EOCD_BYTES)?.checked_add(comment)? == bytes.len()
            && found.replace(offset).is_some()
        {
            return None;
        }
    }
    found
}

/// Detect the ZIP64 locator immediately preceding the ordinary EOCD.
fn has_zip64_locator(bytes: &[u8], eocd: usize) -> bool {
    eocd.checked_sub(20)
        .and_then(|offset| read_u32(bytes, offset))
        == Some(0x0706_4b50)
}

/// Reject ZIP64 extra fields and malformed TLVs in local or central records.
fn safe_extra(bytes: &[u8], offset: usize, len: usize) -> bool {
    let Some(end) = offset.checked_add(len) else {
        return false;
    };
    let Some(extra) = bytes.get(offset..end) else {
        return false;
    };
    let mut cursor = 0usize;
    while cursor < extra.len() {
        let Some(field_id) = read_u16(extra, cursor) else {
            return false;
        };
        let Some(field_len) = read_u16(extra, cursor + 2).map(usize::from) else {
            return false;
        };
        if field_id == 1 {
            return false;
        }
        let Some(next) = cursor
            .checked_add(4)
            .and_then(|start| start.checked_add(field_len))
        else {
            return false;
        };
        if next > extra.len() {
            return false;
        }
        cursor = next;
    }
    true
}

/// Accept stored or deflated entries; descriptors are deflate-only.
fn safe_flags(flags: u16, method: u16) -> bool {
    flags & !0x080e == 0
        && flags & 0x0001 == 0
        && flags & 0x0040 == 0
        && flags & 0x2000 == 0
        && (method == 0 || method == 8)
        && (flags & 0x0008 == 0 || method == 8)
        && (method == 8 || flags & 0x0006 == 0)
}

/// Require an ordinary file, never a directory, volume label, device, or symlink.
fn regular_file(system: u8, attributes: u32) -> bool {
    match system {
        0 => attributes & 0x58 == 0,
        3 => {
            let mode = attributes >> 16;
            mode & 0o170_000 == 0o100_000 && mode & 0o111 == 0 && mode & 0o6_000 == 0
        }
        _ => false,
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let pair = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([pair[0], pair[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let quad = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
}

fn invalid_archive() -> String {
    "baseline_archive_invalid".to_owned()
}

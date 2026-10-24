use super::{BLOCK, MAX_PATH_BYTES, MAX_STAGE_BYTES, StageError};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

pub(super) fn scan_archive(decoded: &mut File, expected: &str) -> Result<File, StageError> {
    decoded
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_tar_archive_read_failed")?;
    let mut payload = tempfile::tempfile().map_err(|_| "cache_tar_temporary_file_failed")?;
    let mut header = [0_u8; BLOCK];
    read_block(decoded, &mut header)?;
    if header.iter().all(|byte| *byte == 0) {
        return Err("cache_tar_member_missing");
    }
    check_checksum(&header)?;
    let (kind, size) = (header[156], parse_number(&header[124..136])?);
    let mut pax_path = None;
    if kind == b'x' {
        if size > 64 * 1024 {
            return Err("cache_tar_metadata_limit");
        }
        let mut body = vec![0_u8; usize::try_from(size).map_err(|_| "cache_tar_metadata_limit")?];
        decoded
            .read_exact(&mut body)
            .map_err(|_| "cache_tar_archive_invalid")?;
        read_padding(decoded, size)?;
        pax_path = Some(parse_pax_path(&body)?);
        read_block(decoded, &mut header)?;
        if header.iter().all(|byte| *byte == 0) {
            return Err("cache_tar_member_missing");
        }
        check_checksum(&header)?;
    }
    if !matches!(header[156], 0 | b'0') {
        return Err("cache_tar_member_type_invalid");
    }
    let size = parse_number(&header[124..136])?;
    if size > MAX_STAGE_BYTES {
        return Err("cache_tar_member_limit");
    }
    let header_path = parse_header_path(&header)?;
    let path = pax_path.unwrap_or(header_path);
    validate_member_path(&path)?;
    if path != expected {
        return Err("cache_tar_member_path_mismatch");
    }
    copy_exact(decoded, &mut payload, size)?;
    read_padding(decoded, size)?;
    read_block(decoded, &mut header)?;
    if !header.iter().all(|byte| *byte == 0) {
        return Err("cache_tar_member_count_invalid");
    }
    read_block(decoded, &mut header)?;
    if !header.iter().all(|byte| *byte == 0) {
        return Err("cache_tar_end_marker_invalid");
    }
    let mut trailing = [0_u8; BLOCK];
    let mut trailing_len = 0_u64;
    loop {
        let count = decoded
            .read(&mut trailing)
            .map_err(|_| "cache_tar_archive_read_failed")?;
        if count == 0 {
            break;
        }
        trailing_len = trailing_len
            .checked_add(u64::try_from(count).map_err(|_| "cache_tar_archive_invalid")?)
            .ok_or("cache_tar_archive_invalid")?;
        if trailing[..count].iter().any(|byte| *byte != 0) {
            return Err("cache_tar_trailing_data_invalid");
        }
    }
    if !trailing_len.is_multiple_of(BLOCK as u64) {
        return Err("cache_tar_trailing_data_invalid");
    }
    payload
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_tar_temporary_file_failed")?;
    Ok(payload)
}

pub(super) fn read_block(reader: &mut File, block: &mut [u8; BLOCK]) -> Result<(), StageError> {
    reader
        .read_exact(block)
        .map_err(|_| "cache_tar_archive_invalid")
}

pub(super) fn check_checksum(header: &[u8; BLOCK]) -> Result<(), StageError> {
    let expected = parse_number(&header[148..156])?;
    let actual = header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                usize::from(b' ')
            } else {
                usize::from(*byte)
            }
        })
        .sum::<usize>();
    if usize::try_from(expected).ok() != Some(actual) {
        return Err("cache_tar_checksum_invalid");
    }
    Ok(())
}

pub(super) fn parse_number(field: &[u8]) -> Result<u64, StageError> {
    if field.first().is_some_and(|byte| byte & 0x80 != 0) {
        if field[0] & 0x40 != 0 {
            return Err("cache_tar_number_invalid");
        }
        let mut value = u64::from(field[0] & 0x7f);
        for byte in &field[1..] {
            value = value
                .checked_mul(256)
                .and_then(|current| current.checked_add(u64::from(*byte)))
                .ok_or("cache_tar_number_invalid")?;
        }
        return Ok(value);
    }
    let trimmed = field
        .iter()
        .copied()
        .skip_while(|byte| matches!(*byte, b' ' | b'\0'))
        .take_while(|byte| !matches!(*byte, b' ' | b'\0'));
    let mut value = 0_u64;
    let mut found = false;
    for byte in trimmed {
        if !(b'0'..=b'7').contains(&byte) {
            return Err("cache_tar_number_invalid");
        }
        value = value
            .checked_mul(8)
            .and_then(|current| current.checked_add(u64::from(byte - b'0')))
            .ok_or("cache_tar_number_invalid")?;
        found = true;
    }
    if !found {
        return Err("cache_tar_number_invalid");
    }
    Ok(value)
}

pub(super) fn parse_header_path(header: &[u8; BLOCK]) -> Result<String, StageError> {
    let name = field_text(&header[..100])?;
    let prefix = field_text(&header[345..500])?;
    if prefix.is_empty() {
        Ok(name)
    } else if name.is_empty() {
        Err("cache_tar_path_invalid")
    } else {
        Ok(format!("{prefix}/{name}"))
    }
}

pub(super) fn field_text(field: &[u8]) -> Result<String, StageError> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    if field[end..].iter().any(|byte| *byte != 0) {
        return Err("cache_tar_header_invalid");
    }
    let text = std::str::from_utf8(&field[..end]).map_err(|_| "cache_tar_path_invalid")?;
    if text.bytes().any(|byte| byte.is_ascii_control()) {
        return Err("cache_tar_path_invalid");
    }
    Ok(text.to_owned())
}

pub(super) fn parse_pax_path(body: &[u8]) -> Result<String, StageError> {
    let mut offset = 0_usize;
    let mut path = None;
    while offset < body.len() {
        let space = body[offset..]
            .iter()
            .position(|byte| *byte == b' ')
            .map(|index| offset + index)
            .ok_or("cache_tar_pax_invalid")?;
        if space == offset || !body[offset..space].iter().all(u8::is_ascii_digit) {
            return Err("cache_tar_pax_invalid");
        }
        let length = std::str::from_utf8(&body[offset..space])
            .map_err(|_| "cache_tar_pax_invalid")?
            .parse::<usize>()
            .map_err(|_| "cache_tar_pax_invalid")?;
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= body.len())
            .ok_or("cache_tar_pax_invalid")?;
        if end <= space + 1 || body[end - 1] != b'\n' {
            return Err("cache_tar_pax_invalid");
        }
        let record = &body[space + 1..end - 1];
        let equals = record
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or("cache_tar_pax_invalid")?;
        let key = &record[..equals];
        let value = &record[equals + 1..];
        if key != b"path" || value.is_empty() || value.len() > MAX_PATH_BYTES {
            return Err("cache_tar_pax_invalid");
        }
        let value = std::str::from_utf8(value).map_err(|_| "cache_tar_pax_invalid")?;
        if value.bytes().any(|byte| byte.is_ascii_control()) || path.is_some() {
            return Err("cache_tar_pax_invalid");
        }
        path = Some(value.to_owned());
        offset = end;
    }
    path.ok_or("cache_tar_pax_invalid")
}

pub(super) fn validate_member_path(path: &str) -> Result<(), StageError> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == 0)
    {
        return Err("cache_tar_path_invalid");
    }
    Ok(())
}

fn copy_exact<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    size: u64,
) -> Result<(), StageError> {
    let mut remaining = size;
    let mut buffer = vec![0_u8; 64 * 1024];
    while remaining > 0 {
        let limit = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| "cache_tar_member_limit")?;
        let count = reader
            .read(&mut buffer[..limit])
            .map_err(|_| "cache_tar_archive_read_failed")?;
        if count == 0 {
            return Err("cache_tar_member_truncated");
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|_| "cache_tar_temporary_file_failed")?;
        remaining -= u64::try_from(count).map_err(|_| "cache_tar_member_limit")?;
    }
    Ok(())
}

pub(super) fn read_padding(reader: &mut File, size: u64) -> Result<(), StageError> {
    let padding = (BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64;
    let mut remaining = padding;
    let mut buffer = [0_u8; BLOCK];
    while remaining > 0 {
        let count = usize::try_from(remaining).map_err(|_| "cache_tar_archive_invalid")?;
        reader
            .read_exact(&mut buffer[..count])
            .map_err(|_| "cache_tar_archive_invalid")?;
        if buffer[..count].iter().any(|byte| *byte != 0) {
            return Err("cache_tar_padding_invalid");
        }
        remaining -= count as u64;
    }
    Ok(())
}

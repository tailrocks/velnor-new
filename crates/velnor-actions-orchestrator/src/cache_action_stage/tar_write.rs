use super::super::stage::StageReader;
use super::format::validate_member_path;
use super::{BLOCK, MAX_STAGE_BYTES, StageError, io_copy_bounded};
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};

pub(super) fn write_single_member(
    reader: &mut StageReader,
    raw: &mut File,
    member_path: &str,
) -> Result<(), StageError> {
    validate_member_path(member_path)?;
    let (mut prefix, mut name) = member_path.rsplit_once('/').unwrap_or(("", member_path));
    let needs_pax = prefix.len() > 155 || name.len() > 100 || name.is_empty();
    if needs_pax {
        let path_record = pax_path_record(member_path)?;
        let extension_size =
            u64::try_from(path_record.len()).map_err(|_| "cache_tar_pax_invalid")?;
        let extension = make_header("PaxHeaders/cache-stage", "", b'x', extension_size)?;
        raw.write_all(&extension)
            .and_then(|()| raw.write_all(&path_record))
            .map_err(|_| "cache_tar_temporary_file_failed")?;
        write_padding(raw, extension_size)?;
        prefix = "";
        name = "cache-stage.payload";
    }
    if name.is_empty() {
        return Err("cache_tar_path_invalid");
    }
    reader.revalidate().map_err(|_| "cache_tar_stage_invalid")?;
    let size = reader
        .seek(SeekFrom::End(0))
        .map_err(|_| "cache_tar_stage_invalid")?;
    if size > MAX_STAGE_BYTES {
        return Err("cache_tar_member_limit");
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_tar_stage_invalid")?;
    let header = make_header(name, prefix, b'0', size)?;
    raw.write_all(&header)
        .map_err(|_| "cache_tar_temporary_file_failed")?;
    io_copy_bounded(reader, raw, MAX_STAGE_BYTES)?;
    write_padding(raw, size)?;
    write_end_markers(raw)
}

pub(super) fn make_header(
    name: &str,
    prefix: &str,
    kind: u8,
    size: u64,
) -> Result<[u8; BLOCK], StageError> {
    let mut header = [0_u8; BLOCK];
    put_field(&mut header[..100], name.as_bytes())?;
    put_octal(&mut header[100..108], 0o600)?;
    put_octal(&mut header[108..116], 0)?;
    put_octal(&mut header[116..124], 0)?;
    put_number(&mut header[124..136], size)?;
    put_octal(&mut header[136..148], 0)?;
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    put_field(&mut header[345..500], prefix.as_bytes())?;
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    let checksum_text = format!("{checksum:06o}");
    if checksum_text.len() != 6 {
        return Err("cache_tar_header_invalid");
    }
    header[148..154].copy_from_slice(checksum_text.as_bytes());
    header[154] = 0;
    header[155] = b' ';
    Ok(header)
}

pub(super) fn write_padding(raw: &mut File, size: u64) -> Result<(), StageError> {
    let padding = (BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64;
    if padding > 0 {
        let zeros = [0_u8; BLOCK];
        raw.write_all(&zeros[..usize::try_from(padding).map_err(|_| "cache_tar_member_limit")?])
            .map_err(|_| "cache_tar_temporary_file_failed")?;
    }
    Ok(())
}

pub(super) fn pax_path_record(path: &str) -> Result<Vec<u8>, StageError> {
    let record = format!("path={path}\n");
    for length in record.len()..=record.len() + 24 {
        let line = format!("{length} {record}");
        if line.len() == length {
            return Ok(line.into_bytes());
        }
    }
    Err("cache_tar_pax_invalid")
}

pub(super) fn write_end_markers(raw: &mut File) -> Result<(), StageError> {
    raw.write_all(&[0_u8; BLOCK * 2])
        .map_err(|_| "cache_tar_temporary_file_failed")?;
    Ok(())
}

pub(super) fn put_field(field: &mut [u8], text: &[u8]) -> Result<(), StageError> {
    if text.len() > field.len() {
        return Err("cache_tar_path_invalid");
    }
    field.fill(0);
    field[..text.len()].copy_from_slice(text);
    Ok(())
}

pub(super) fn put_octal(field: &mut [u8], value: u64) -> Result<(), StageError> {
    let width = field.len().saturating_sub(1);
    let text = format!("{value:0width$o}");
    if text.len() > width {
        return Err("cache_tar_number_invalid");
    }
    field.fill(b'0');
    field[..text.len()].copy_from_slice(text.as_bytes());
    *field.last_mut().ok_or("cache_tar_number_invalid")? = 0;
    Ok(())
}

pub(super) fn put_number(field: &mut [u8], value: u64) -> Result<(), StageError> {
    if value < 8_u64.pow(u32::try_from(field.len().saturating_sub(1)).unwrap_or(0)) {
        return put_octal(field, value);
    }
    let available_bits = field.len().saturating_mul(8).saturating_sub(2);
    if field.len() < 9
        || (available_bits < u64::BITS as usize && value >= (1_u64 << available_bits))
    {
        return Err("cache_tar_number_invalid");
    }
    field.fill(0);
    let mut remaining = value;
    for byte in field.iter_mut().rev() {
        *byte = u8::try_from(remaining % 256).map_err(|_| "cache_tar_number_invalid")?;
        remaining >>= 8;
    }
    field[0] |= 0x80;
    Ok(())
}

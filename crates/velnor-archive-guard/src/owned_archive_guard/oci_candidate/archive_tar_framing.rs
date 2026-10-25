use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

use super::{OciCandidateError, Result};

const BLOCK: usize = 512;
const EXTENSION_LIMIT: u64 = 1024 * 1024;
const EXTENSION_TOTAL_LIMIT: u64 = 4 * 1024 * 1024;

pub(super) fn scan_archive_extensions(file: &mut File, archive_size: u64) -> Result<()> {
    let mut first = [0; 6];
    let read = read_bounded(file, &mut first, archive_size)?;
    if read != first.len() {
        return Err(OciCandidateError("archive_read"));
    }
    if first.starts_with(&[0x1f, 0x8b])
        || first.starts_with(b"BZh")
        || first.starts_with(b"\xfd7zXZ\0")
    {
        return Err(OciCandidateError("archive_compressed_tar"));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| OciCandidateError("archive_seek"))?;
    scan_tar_extensions(file, archive_size)
}

fn scan_tar_extensions(file: &mut File, archive_size: u64) -> Result<()> {
    let mut extension_total = 0_u64;
    let mut header = [0; BLOCK];
    loop {
        let mut filled = 0;
        while filled < header.len() {
            let read = read_bounded(file, &mut header[filled..], archive_size)?;
            if read == 0 {
                return Err(OciCandidateError(if filled == 0 {
                    "tar_scan_terminator_missing"
                } else {
                    "tar_scan_truncated_header"
                }));
            }
            filled += read;
        }
        if header.iter().all(|byte| *byte == 0) {
            return validate_terminator_and_padding(file, archive_size);
        }
        let size = tar_size(&header[124..136])?;
        if matches!(header[156], b'x' | b'g' | b'L' | b'K') {
            if size > EXTENSION_LIMIT {
                return Err(OciCandidateError("tar_extension_size_limit"));
            }
            extension_total = extension_total
                .checked_add(size)
                .filter(|total| *total <= EXTENSION_TOTAL_LIMIT)
                .ok_or(OciCandidateError("tar_extension_total_limit"))?;
        }
        let padded = size
            .checked_add(BLOCK as u64 - 1)
            .ok_or(OciCandidateError("tar_member_size_overflow"))?
            / BLOCK as u64
            * BLOCK as u64;
        let next = file
            .stream_position()
            .map_err(|_| OciCandidateError("tar_scan_seek"))?
            .checked_add(padded)
            .filter(|position| *position <= archive_size)
            .ok_or(OciCandidateError("tar_scan_truncated_body"))?;
        file.seek(SeekFrom::Start(next))
            .map_err(|_| OciCandidateError("tar_scan_seek"))?;
    }
}

fn validate_terminator_and_padding(file: &mut File, archive_size: u64) -> Result<()> {
    let mut second = [0; BLOCK];
    let mut filled = 0;
    while filled < second.len() {
        let read = read_bounded(file, &mut second[filled..], archive_size)?;
        if read == 0 {
            return Err(OciCandidateError("tar_scan_terminator_truncated"));
        }
        filled += read;
    }
    if second.iter().any(|byte| *byte != 0) {
        return Err(OciCandidateError("tar_scan_terminator_invalid"));
    }

    let mut padding = vec![0; 64 * 1024];
    loop {
        let read = read_bounded(file, &mut padding, archive_size)?;
        if read == 0 {
            return Ok(());
        }
        if padding[..read].iter().any(|byte| *byte != 0) {
            return Err(OciCandidateError("tar_scan_trailing_data"));
        }
    }
}

fn read_bounded(file: &mut File, buffer: &mut [u8], archive_size: u64) -> Result<usize> {
    let position = file
        .stream_position()
        .map_err(|_| OciCandidateError("tar_scan_seek"))?;
    let remaining = archive_size.saturating_sub(position);
    let buffer_size =
        u64::try_from(buffer.len()).map_err(|_| OciCandidateError("tar_scan_read"))?;
    let amount = usize::try_from(remaining.min(buffer_size))
        .map_err(|_| OciCandidateError("tar_scan_read"))?;
    if amount == 0 {
        return Ok(0);
    }
    file.read(&mut buffer[..amount])
        .map_err(|_| OciCandidateError("tar_scan_read"))
}

fn tar_size(field: &[u8]) -> Result<u64> {
    if field.first().is_some_and(|byte| byte & 0x80 != 0) {
        let mut bytes = field.to_vec();
        bytes[0] &= 0x7f;
        return bytes.into_iter().try_fold(0_u64, |value, byte| {
            value
                .checked_mul(256)
                .and_then(|value| value.checked_add(u64::from(byte)))
                .ok_or(OciCandidateError("tar_size_overflow"))
        });
    }
    let text = field
        .iter()
        .copied()
        .take_while(|byte| *byte != 0 && *byte != b' ')
        .collect::<Vec<_>>();
    if text.is_empty() {
        return Ok(0);
    }
    text.iter().try_fold(0_u64, |value, byte| {
        if !(b'0'..=b'7').contains(byte) {
            return Err(OciCandidateError("tar_size_invalid"));
        }
        value
            .checked_mul(8)
            .and_then(|value| value.checked_add(u64::from(*byte - b'0')))
            .ok_or(OciCandidateError("tar_size_overflow"))
    })
}

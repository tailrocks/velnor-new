use std::error::Error;
use std::io::{self, Write};

use flate2::Compression as GzipLevel;
use flate2::write::GzEncoder;

use super::validate;
pub(super) fn header(name: &str, kind: u8, size: u64) -> [u8; 512] {
    let mut block = [0_u8; 512];
    let name_bytes = name.as_bytes();
    block[..name_bytes.len()].copy_from_slice(name_bytes);
    write_octal(&mut block[100..108], 0o644);
    write_octal(&mut block[108..116], 0);
    write_octal(&mut block[116..124], 0);
    write_octal(&mut block[124..136], size);
    write_octal(&mut block[136..148], 0);
    block[148..156].fill(b' ');
    block[156] = kind;
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    let checksum = block.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    let formatted = format!("{checksum:06o}\0 ");
    block[148..156].copy_from_slice(formatted.as_bytes());
    block
}

pub(super) fn write_octal(field: &mut [u8], value: u64) {
    let text = format!("{value:o}");
    let start = field.len() - text.len() - 1;
    field.fill(b'0');
    field[start..start + text.len()].copy_from_slice(text.as_bytes());
    field[field.len() - 1] = 0;
}

pub(super) fn append_member(
    archive: &mut Vec<u8>,
    name: &str,
    kind: u8,
    body: &[u8],
) -> Result<(), io::Error> {
    let size = u64::try_from(body.len()).map_err(io::Error::other)?;
    archive.extend_from_slice(&header(name, kind, size));
    archive.extend_from_slice(body);
    let padding = (512 - body.len() % 512) % 512;
    archive.resize(archive.len() + padding, 0);
    Ok(())
}

pub(super) fn append_member_with_padding(
    archive: &mut Vec<u8>,
    name: &str,
    kind: u8,
    body: &[u8],
    padding_prefix: &[u8],
) -> Result<(), io::Error> {
    let size = u64::try_from(body.len()).map_err(io::Error::other)?;
    let padding = (512 - body.len() % 512) % 512;
    if padding_prefix.len() > padding {
        return Err(io::Error::other("padding prefix exceeds tar padding"));
    }
    archive.extend_from_slice(&header(name, kind, size));
    archive.extend_from_slice(body);
    archive.extend_from_slice(padding_prefix);
    archive.resize(archive.len() + padding - padding_prefix.len(), 0);
    Ok(())
}

pub(super) fn finish_archive(archive: &mut Vec<u8>) {
    archive.resize(archive.len() + 1024, 0);
}

pub(super) fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let content = format!("{key}={value}\n");
    let mut length = content.len() + 2;
    loop {
        let next = length.to_string().len() + 1 + content.len();
        if next == length {
            return format!("{length} {content}").into_bytes();
        }
        length = next;
    }
}

pub(super) fn gzip(archive: &[u8]) -> Result<Vec<u8>, io::Error> {
    let mut encoder = GzEncoder::new(Vec::new(), GzipLevel::default());
    encoder.write_all(archive)?;
    encoder.finish()
}

pub(super) fn assert_rejected<R: std::io::Read>(
    archive: R,
    mode: &str,
    needle: &str,
) -> Result<(), Box<dyn Error>> {
    let problem = match validate(archive, mode) {
        Ok(()) => return Err(io::Error::other("archive unexpectedly passed").into()),
        Err(problem) => problem,
    };
    assert!(problem.contains(needle), "unexpected error: {problem}");
    Ok(())
}

pub(super) fn assert_rejected_tar(
    archive: &[u8],
    mode: &str,
    needle: &str,
) -> Result<(), Box<dyn Error>> {
    assert_rejected(gzip(archive)?.as_slice(), mode, needle)
}

pub(super) fn assert_rejected_raw_tar(
    archive: &[u8],
    mode: &str,
    needle: &str,
) -> Result<(), Box<dyn Error>> {
    assert_rejected(archive, mode, needle)
}

pub(super) fn validate_tar(archive: &[u8], mode: &str) -> Result<(), Box<dyn Error>> {
    validate(gzip(archive)?.as_slice(), mode).map_err(io::Error::other)?;
    Ok(())
}

mod headers_tests;
mod limits_tests;
mod streams_tests;

use super::super::{BLOCK, put_field, put_octal};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

pub(super) fn member(path: &str, kind: u8, body: &[u8]) -> Vec<u8> {
    let mut header = [0_u8; BLOCK];
    put_field(&mut header[..100], path.as_bytes()).expect("test path fits");
    put_octal(&mut header[100..108], 0o600).expect("mode fits");
    put_octal(&mut header[108..116], 0).expect("uid fits");
    put_octal(&mut header[116..124], 0).expect("gid fits");
    put_octal(&mut header[124..136], body.len() as u64).expect("size fits");
    put_octal(&mut header[136..148], 0).expect("mtime fits");
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    let checksum = format!("{checksum:06o}");
    header[148..154].copy_from_slice(checksum.as_bytes());
    header[154] = 0;
    header[155] = b' ';

    let mut archive = header.to_vec();
    archive.extend_from_slice(body);
    archive.resize(archive.len().div_ceil(BLOCK) * BLOCK, 0);
    archive
}

pub(super) fn finish(mut archive: Vec<u8>) -> std::fs::File {
    archive.extend_from_slice(&[0; BLOCK * 2]);
    let mut file = tempfile::tempfile().expect("temporary archive");
    file.write_all(&archive).expect("write archive");
    file.seek(SeekFrom::Start(0)).expect("rewind archive");
    file
}

pub(super) fn archive_path(runner_temp: &Path) -> std::path::PathBuf {
    runner_temp.join("cache.tar")
}

pub(super) fn pax_path(path: &str) -> Vec<u8> {
    pax_record("path", path)
}

pub(super) fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let record = format!("{key}={value}\n");
    for length in record.len()..=record.len() + 16 {
        let line = format!("{length} {record}");
        if line.len() == length {
            return line.into_bytes();
        }
    }
    panic!("PAX record length converges");
}

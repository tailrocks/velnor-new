use super::*;
use std::io::{Cursor, Write};
use std::time::Duration;
use zip::write::SimpleFileOptions;

struct OversizedEvidenceArchive;

impl std::io::Read for OversizedEvidenceArchive {
    fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

impl std::io::Seek for OversizedEvidenceArchive {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        match position {
            std::io::SeekFrom::End(_) => Ok(ZipProfile::Evidence.compressed() + 1),
            std::io::SeekFrom::Start(offset) => Ok(offset),
            std::io::SeekFrom::Current(_) => Ok(0),
        }
    }
}

fn archive(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body, mode) in entries {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(*mode),
            )
            .expect("ZIP entry");
        writer.write_all(body).expect("ZIP body");
    }
    writer.finish().expect("ZIP finish").into_inner()
}

fn header_offsets(bytes: &[u8], signature: &[u8]) -> Vec<usize> {
    bytes
        .windows(signature.len())
        .enumerate()
        .filter_map(|(index, window)| (window == signature).then_some(index))
        .collect()
}

fn duplicate_names(mut bytes: Vec<u8>) -> Vec<u8> {
    let locals = header_offsets(&bytes, b"PK\x03\x04");
    let centrals = header_offsets(&bytes, b"PK\x01\x02");
    assert_eq!(locals.len(), 2);
    assert_eq!(centrals.len(), 2);
    let first = &b"same/one"[..];
    let second = &b"same/two"[..];
    assert_eq!(first.len(), second.len());
    bytes[locals[1] + 30..locals[1] + 30 + second.len()].copy_from_slice(first);
    bytes[centrals[1] + 46..centrals[1] + 46 + second.len()].copy_from_slice(first);
    bytes
}

fn with_unix_type(mut bytes: Vec<u8>, mode: u32) -> Vec<u8> {
    let central = header_offsets(&bytes, b"PK\x01\x02")[0];
    bytes[central + 5] = 3;
    bytes[central + 38..central + 42].copy_from_slice(&(mode << 16).to_le_bytes());
    bytes
}

fn validate(bytes: &[u8]) -> Result<(), OrchestratorError> {
    let fixture = write_fixture(bytes);
    let mut file = fixture
        .reopen()
        .map_err(|_| crate::internal::internal("fixture_read"))?;
    let deadline = CheckDeadline::after(Duration::from_secs(5)).expect("deadline");
    validate_evidence_archive(&mut file, deadline).map(|_| ())
}

fn write_fixture(bytes: &[u8]) -> tempfile::NamedTempFile {
    use std::io::Write as _;
    let mut file = tempfile::NamedTempFile::new().expect("ZIP fixture");
    file.write_all(bytes).expect("ZIP bytes");
    file
}

#[test]
fn evidence_profile_accepts_nested_files_and_explicit_directories() {
    let nested = archive(&[("reports/attempt.json", b"{}", 0o100_644)]);
    validate(&nested).expect("safe nested path");
    let mut measured = write_fixture(&nested);
    let unpacked = validate_evidence_archive(
        measured.as_file_mut(),
        CheckDeadline::after(Duration::from_secs(5)).expect("deadline"),
    )
    .expect("bounded member bytes");
    assert_eq!(unpacked, 2);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_directory(
            "reports/",
            SimpleFileOptions::default().unix_permissions(0o40755),
        )
        .expect("directory");
    writer
        .start_file(
            "reports/attempt.json",
            SimpleFileOptions::default().unix_permissions(0o100_644),
        )
        .expect("file");
    writer.write_all(b"{}").expect("body");
    validate(&writer.finish().expect("ZIP").into_inner()).expect("explicit directory");

    let mut native = Cursor::new(nested);
    assert!(reader::open(&mut native, ZipProfile::Recovery).is_err());
}

#[test]
fn evidence_profile_rejects_fake_truncated_and_path_escaping_archives() {
    assert!(validate(b"PK\x03\x04").is_err());
    let valid = archive(&[("safe/report.json", b"{}", 0o100_644)]);
    assert!(validate(&valid[..valid.len() - 9]).is_err());
    let mut truncated_body = archive(&[("safe/report.json", b"body", 0o100_644)]);
    let local = header_offsets(&truncated_body, b"PK\x03\x04")[0];
    let name_len = usize::from(u16::from_le_bytes([
        truncated_body[local + 26],
        truncated_body[local + 27],
    ]));
    let extra_len = usize::from(u16::from_le_bytes([
        truncated_body[local + 28],
        truncated_body[local + 29],
    ]));
    truncated_body.remove(local + 30 + name_len + extra_len + 3);
    assert!(validate(&truncated_body).is_err());
    for name in [
        "../escape",
        "/absolute",
        "C:/drive",
        "safe/../../escape",
        "safe//empty",
        "safe/./dot",
        "safe\\escape",
        "safe/",
    ] {
        assert!(
            validate(&archive(&[(name, b"body", 0o100_644)])).is_err(),
            "{name}"
        );
    }
}

#[test]
fn evidence_profile_rejects_duplicate_alias_special_and_encrypted_entries() {
    let duplicate = duplicate_names(archive(&[
        ("same/one", b"one", 0o100_644),
        ("same/two", b"two", 0o100_644),
    ]));
    assert!(validate(&duplicate).is_err());
    let file_dir_collision = archive(&[
        ("same", b"file", 0o100_644),
        ("same/child", b"nested", 0o100_644),
    ]);
    assert!(validate(&file_dir_collision).is_err());
    let symlink = with_unix_type(archive(&[("link", b"target", 0o100_644)]), 0o120_777);
    assert!(validate(&symlink).is_err());
    let special = with_unix_type(archive(&[("device", b"data", 0o100_644)]), 0o060_600);
    assert!(validate(&special).is_err());

    let mut encrypted = archive(&[("secret", b"secret", 0o100_644)]);
    for signature in [b"PK\x03\x04".as_slice(), b"PK\x01\x02".as_slice()] {
        let offset = encrypted
            .windows(signature.len())
            .position(|window| window == signature)
            .expect("ZIP header");
        let flags = offset + if signature[2] == 3 { 6 } else { 8 };
        encrypted[flags] |= 1;
    }
    assert!(validate(&encrypted).is_err());

    let mut aliased_local = archive(&[
        ("same/one", b"one", 0o100_644),
        ("same/two", b"two", 0o100_644),
    ]);
    let locals = header_offsets(&aliased_local, b"PK\x03\x04");
    aliased_local[locals[1] + 30..locals[1] + 30 + b"same/one".len()].copy_from_slice(b"same/one");
    assert!(validate(&aliased_local).is_err());

    let mut overlapping = archive(&[
        ("same/one", b"one", 0o100_644),
        ("same/two", b"two", 0o100_644),
    ]);
    let locals = header_offsets(&overlapping, b"PK\x03\x04");
    let centrals = header_offsets(&overlapping, b"PK\x01\x02");
    let first_offset = u32::try_from(locals[0]).expect("local offset");
    overlapping[centrals[1] + 42..centrals[1] + 46].copy_from_slice(&first_offset.to_le_bytes());
    assert!(validate(&overlapping).is_err());
}

#[test]
fn evidence_profile_streams_crc_to_eof_and_rejects_declared_expansion_and_expired_deadline() {
    let mut bad_crc = archive(&[("report", b"payload", 0o100_644)]);
    let name_len = usize::from(u16::from_le_bytes([bad_crc[26], bad_crc[27]]));
    let extra_len = usize::from(u16::from_le_bytes([bad_crc[28], bad_crc[29]]));
    bad_crc[30 + name_len + extra_len] ^= 1;
    assert!(validate(&bad_crc).is_err());

    for (entries, declared) in [
        (
            vec![("report", b"x".as_slice(), 0o100_644)],
            ZipProfile::Evidence.member_unpacked() + 1,
        ),
        (
            ["a", "b", "c", "d", "e"]
                .into_iter()
                .map(|name| (name, b"x".as_slice(), 0o100_644))
                .collect::<Vec<_>>(),
            250 * 1024 * 1024,
        ),
    ] {
        let mut expanded = archive(&entries);
        let declared = u32::try_from(declared).expect("ZIP32 evidence bound");
        for local in header_offsets(&expanded, b"PK\x03\x04") {
            expanded[local + 22..local + 26].copy_from_slice(&declared.to_le_bytes());
        }
        for central in header_offsets(&expanded, b"PK\x01\x02") {
            expanded[central + 24..central + 28].copy_from_slice(&declared.to_le_bytes());
        }
        assert!(validate(&expanded).is_err());
    }

    let mut file = write_fixture(&archive(&[("report", b"x", 0o100_644)]));
    let deadline = CheckDeadline::from_start(
        std::time::Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("past instant"),
        Duration::ZERO,
    )
    .expect("expired deadline");
    assert!(validate_evidence_archive(file.as_file_mut(), deadline).is_err());
}

#[test]
fn evidence_profile_rejects_oversized_member_count_and_compressed_stream() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..=ZipProfile::Evidence.entries() {
        writer
            .start_file(
                format!("entry-{index}"),
                SimpleFileOptions::default().unix_permissions(0o100_644),
            )
            .expect("ZIP entry");
    }
    assert!(validate(&writer.finish().expect("ZIP").into_inner()).is_err());

    let deadline = CheckDeadline::after(Duration::from_secs(5)).expect("deadline");
    assert!(
        deadline
            .scope(|| validate_evidence_reader(OversizedEvidenceArchive))
            .expect("scope")
            .is_err()
    );
}

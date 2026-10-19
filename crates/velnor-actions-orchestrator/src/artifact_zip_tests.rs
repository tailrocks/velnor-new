use super::*;
use std::{fs, io::Cursor, time::Duration};
use zip::{ZipArchive, write::SimpleFileOptions};

fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(2)).expect("deadline")
}
fn read_fixture_manifest(
    bytes: &[u8],
    name: &str,
    max_bytes: u64,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    deadline
        .scope(|| read_manifest_in_scope(bytes, name, max_bytes))
        .map_err(|_| invalid("deadline"))?
}
fn extract_fixture(
    bytes: &[u8],
    destination: &Path,
    expected: &[ExpectedZipEntry],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    deadline
        .scope(|| extract_in_scope(bytes, destination, expected))
        .map_err(|_| invalid("deadline"))?
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in entries {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o644),
            )
            .expect("file");
        writer.write_all(body).expect("body");
    }
    writer.finish().expect("ZIP").into_inner()
}
pub(super) fn expected() -> Vec<ExpectedZipEntry> {
    [
        ("proof.json", b"native-proof".as_slice()),
        ("payload", b"payload-body".as_slice()),
    ]
    .into_iter()
    .map(|(name, body)| ExpectedZipEntry {
        name: name.into(),
        size: Some(body.len() as u64),
        sha256: Some(super::super::github::sha256(body)),
        max_bytes: 1024,
        unix_permissions: Some(0o644),
    })
    .collect()
}
pub(super) fn bytes() -> Vec<u8> {
    zip(&[
        ("proof.json", b"native-proof"),
        ("payload", b"payload-body"),
    ])
}
fn assert_clean_failure(bytes: &[u8], expected: &[ExpectedZipEntry]) {
    let dir = tempfile::tempdir().expect("output");
    assert!(extract_fixture(bytes, dir.path(), expected, deadline()).is_err());
    assert!(dir.path().is_dir(), "caller-owned directory survives");
    assert_eq!(
        fs::read_dir(dir.path()).expect("output").count(),
        0,
        "owned partial files removed"
    );
}

#[test]
fn exact_closed_inventory_reads_manifest_and_streams_bytes_through_crc_eof() {
    let dir = tempfile::tempdir().expect("output");
    let bytes = bytes();
    assert_eq!(
        read_fixture_manifest(&bytes, "proof.json", 1024, deadline()).expect("proof"),
        b"native-proof"
    );
    extract_fixture(&bytes, dir.path(), &expected(), deadline()).expect("exact files");
    assert_eq!(
        fs::read(dir.path().join("payload")).expect("payload"),
        b"payload-body"
    );
    assert_eq!(fs::read_dir(dir.path()).expect("inventory").count(), 2);
}

#[test]
fn manifest_read_checks_later_duplicate_names_before_returning_any_manifest() {
    let mut bytes = zip(&[
        ("proof.json", b"native-proof"),
        ("later.one", b"one"),
        ("later.two", b"one"),
    ]);
    for i in 0..=bytes.len() - 9 {
        if &bytes[i..i + 9] == b"later.two" {
            bytes[i + 6..i + 9].copy_from_slice(b"one");
        }
    }
    assert!(read_fixture_manifest(&bytes, "proof.json", 1024, deadline()).is_err());
}

#[test]
fn flat_paths_exact_members_permissions_sizes_and_hashes_fail_closed() {
    for path in [
        "../payload",
        "sub/payload",
        "sub\\payload",
        "C:payload",
        ".",
        "..",
    ] {
        let bad = zip(&[("proof.json", b"native-proof"), (path, b"payload-body")]);
        assert!(read_fixture_manifest(&bad, "proof.json", 1024, deadline()).is_err());
        assert_clean_failure(&bad, &expected());
    }
    for mutate in [
        |e: &mut Vec<ExpectedZipEntry>| e[1].size = Some(1),
        |e: &mut Vec<ExpectedZipEntry>| e[1].sha256 = Some("b".repeat(64)),
        |e: &mut Vec<ExpectedZipEntry>| e[1].unix_permissions = Some(0o640),
        |e: &mut Vec<ExpectedZipEntry>| e[1].name = "missing".into(),
        |e: &mut Vec<ExpectedZipEntry>| e[1].max_bytes = 1,
    ] {
        let mut e = expected();
        mutate(&mut e);
        assert_clean_failure(&bytes(), &e);
    }
    assert_clean_failure(&bytes(), &expected()[..1]);
}

#[test]
fn nonregular_modes_and_crc_corruption_cannot_leave_partial_output() {
    for mode in [0o120_777_u32, 0o020_600, 0o060_600, 0o010_600] {
        let mut bytes = bytes();
        let at = bytes
            .windows(4)
            .position(|b| b == b"PK\x01\x02")
            .expect("central");
        bytes[at + 38..at + 42].copy_from_slice(&(mode << 16).to_le_bytes());
        assert!(read_fixture_manifest(&bytes, "proof.json", 1024, deadline()).is_err());
        assert_clean_failure(&bytes, &expected());
    }
    let mut corrupt = bytes();
    let at = corrupt
        .windows(12)
        .position(|b| b == b"payload-body")
        .expect("stored bytes");
    corrupt[at] = b'X';
    assert_clean_failure(&corrupt, &expected());
}

#[test]
fn header_size_lies_and_total_inflation_bounds_are_checked_before_publication() {
    let mut bytes = bytes();
    for at in 0..bytes.len().saturating_sub(30) {
        if &bytes[at..at + 4] == b"PK\x03\x04" {
            bytes[at + 22..at + 26].copy_from_slice(&1_u32.to_le_bytes());
        }
        if &bytes[at..at + 4] == b"PK\x01\x02" {
            bytes[at + 24..at + 28].copy_from_slice(&1_u32.to_le_bytes());
        }
    }
    assert!(read_fixture_manifest(&bytes, "proof.json", 1024, deadline()).is_err());
    assert_clean_failure(&bytes, &expected());
    let mut huge = super::tests::bytes();
    for at in 0..huge.len().saturating_sub(30) {
        if &huge[at..at + 4] == b"PK\x03\x04" {
            huge[at + 22..at + 26].copy_from_slice(&0xffff_fffe_u32.to_le_bytes());
        }
        if &huge[at..at + 4] == b"PK\x01\x02" {
            huge[at + 24..at + 28].copy_from_slice(&0xffff_fffe_u32.to_le_bytes());
        }
    }
    assert!(read_fixture_manifest(&huge, "proof.json", 1024, deadline()).is_err());
}

#[test]
fn individually_consistent_nested_local_headers_still_reject_overlapping_ranges() {
    let inner = zip(&[("inner", b"inner-body")]);
    let central = inner
        .windows(4)
        .position(|b| b == b"PK\x01\x02")
        .expect("inner central");
    let embedded = &inner[..central];
    let mut bytes = zip(&[
        ("proof.json", b"native-proof"),
        ("outer", embedded),
        ("inner", b"inner-body"),
    ]);
    let offsets = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(i, b)| (b == b"PK\x03\x04").then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(
        offsets.len(),
        4,
        "inner header is embedded in outer payload"
    );
    let target = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(i, b)| (b == b"PK\x01\x02").then_some(i))
        .next_back()
        .expect("last central");
    bytes[target + 42..target + 46]
        .copy_from_slice(&u32::try_from(offsets[2]).expect("small ZIP").to_le_bytes());
    let mut raw = ZipArchive::new(Cursor::new(&bytes)).expect("metadata");
    for i in 0..raw.len() {
        raw.by_index_with_options(
            i,
            zip::read::ZipReadOptions::new().enforce_header_consistency(true),
        )
        .expect("individual headers match");
    }
    assert!(raw.has_overlapping_entries().expect("ranges"));
    assert!(read_fixture_manifest(&bytes, "proof.json", 1024, deadline()).is_err());
}

#[test]
fn expired_outer_operation_blocks_nested_recovery_and_zip_without_new_clock() {
    let start = std::time::Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("one second before test clock");
    let outer = CheckDeadline::from_start(start, Duration::from_millis(1)).expect("expired");
    let executed = std::cell::Cell::new(false);
    let result = outer.scope(|| {
        within_operation(|| {
            executed.set(true);
            Ok(())
        })
    });
    assert!(result.is_err());
    assert!(!executed.get());
    let dir = tempfile::tempdir().expect("output");
    assert!(extract_fixture(&bytes(), dir.path(), &expected(), outer).is_err());
    assert_eq!(fs::read_dir(dir.path()).expect("empty").count(), 0);
}

#[test]
fn excessive_entry_inventory_is_rejected_before_manifest_read() {
    let names = (0..=MAX_ENTRIES)
        .map(|n| format!("entry-{n:04}"))
        .collect::<Vec<_>>();
    let entries = names
        .iter()
        .map(|name| (name.as_str(), b"x".as_slice()))
        .collect::<Vec<_>>();
    assert!(read_fixture_manifest(&zip(&entries), "entry-0000", 1024, deadline()).is_err());
}

const PINNED_UPLOAD_ZIP: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-preview/upload-artifact-v7.0.2-modes.zip"
));
const PINNED_UPLOAD_BODY: &[u8] = b"identical unprivileged fixture bytes\n";

fn pinned_upload_expectations() -> Vec<ExpectedZipEntry> {
    [("mode-600.txt", 0o600), ("mode-644.txt", 0o644)]
        .into_iter()
        .map(|(name, permissions)| ExpectedZipEntry {
            name: name.into(),
            size: Some(PINNED_UPLOAD_BODY.len() as u64),
            sha256: Some(super::super::github::sha256(PINNED_UPLOAD_BODY)),
            max_bytes: 1024,
            unix_permissions: Some(permissions),
        })
        .collect()
}

#[test]
fn pinned_upload_streaming_zip_preserves_modes_and_admits_exact_data_and_headers() {
    assert_eq!(
        super::super::github::sha256(PINNED_UPLOAD_ZIP),
        "9ebbbf81eaa69fa82acac34953ac5e623578fff91c8b038d150cbefe14a326b1"
    );
    assert_eq!(
        PINNED_UPLOAD_ZIP
            .windows(4)
            .filter(|b| *b == b"PK\x07\x08")
            .count(),
        2
    );
    for (offset, _) in PINNED_UPLOAD_ZIP
        .windows(4)
        .enumerate()
        .filter(|(_, b)| *b == b"PK\x03\x04")
    {
        let flags =
            u16::from_le_bytes([PINNED_UPLOAD_ZIP[offset + 6], PINNED_UPLOAD_ZIP[offset + 7]]);
        assert_eq!(
            flags & 8,
            8,
            "each local header uses a streaming data descriptor"
        );
    }
    let mut archive = ZipArchive::new(Cursor::new(PINNED_UPLOAD_ZIP)).expect("actual bundled ZIP");
    archive
        .validate_header_consistency()
        .expect("streaming headers");
    assert_eq!(archive.len(), 2);
    for (name, mode) in [("mode-600.txt", 0o100_600), ("mode-644.txt", 0o100_644)] {
        let file = archive.by_name(name).expect("exact entry");
        assert_eq!(file.unix_mode(), Some(mode));
        assert_eq!(file.compression(), zip::CompressionMethod::Deflated);
        assert_eq!(file.crc32(), 0x4a47_7490);
        assert_eq!(
            read_fixture_manifest(PINNED_UPLOAD_ZIP, name, 1024, deadline()).expect("CRC EOF"),
            PINNED_UPLOAD_BODY
        );
    }
    let dir = tempfile::tempdir().expect("output");
    extract_fixture(
        PINNED_UPLOAD_ZIP,
        dir.path(),
        &pinned_upload_expectations(),
        deadline(),
    )
    .expect("actual format");
    for name in ["mode-600.txt", "mode-644.txt"] {
        assert_eq!(
            fs::read(dir.path().join(name)).expect("actual data"),
            PINNED_UPLOAD_BODY
        );
    }
    assert_eq!(
        fs::read_dir(dir.path()).expect("exact two files").count(),
        2
    );
}

#[test]
fn pinned_upload_rejects_private_mode_for_native_upload_and_detects_crc_corruption() {
    let mut expected = pinned_upload_expectations();
    expected[0].unix_permissions = Some(0o644);
    let dir = tempfile::tempdir().expect("output");
    let error = extract_fixture(PINNED_UPLOAD_ZIP, dir.path(), &expected, deadline())
        .expect_err("0600 cannot satisfy native 0644 upload contract");
    assert!(error.to_string().contains("file_type_size_mode"), "{error}");
    assert!(dir.path().is_dir());
    assert_eq!(
        fs::read_dir(dir.path()).expect("no partial output").count(),
        0
    );

    let mut corrupt = PINNED_UPLOAD_ZIP.to_vec();
    let central = corrupt
        .windows(4)
        .position(|bytes| bytes == b"PK\x01\x02")
        .expect("central");
    let descriptor = corrupt
        .windows(4)
        .position(|bytes| bytes == b"PK\x07\x08")
        .expect("stream descriptor");
    // Both metadata copies agree with one another, but not with actual data.
    corrupt[central + 16] ^= 1;
    corrupt[descriptor + 4] ^= 1;
    assert!(read_fixture_manifest(&corrupt, "mode-600.txt", 1024, deadline()).is_err());
    assert_clean_failure(&corrupt, &pinned_upload_expectations());
}

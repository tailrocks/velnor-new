use super::super::*;

fn eocd(bytes: &[u8]) -> usize {
    bytes
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .expect("EOCD")
}

fn write_zip(root: &Path, bytes: &[u8], name: &str) -> PathBuf {
    let archive = root.join(name);
    fs::write(&archive, bytes).expect("write ZIP fixture");
    archive
}

fn assert_rejected(root: &Path, bytes: &[u8], name: &str) {
    let archive = write_zip(root, bytes, name);
    let destination = root.join("rejected-prefix");
    assert!(
        extract(&archive, &destination, "https://example.test/tool.zip").is_err(),
        "unexpectedly admitted {name}"
    );
    assert!(!destination.exists());
}

fn empty_zip64(entries_on_disk: u64, entries_total: u64, extension: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PK\x06\x06");
    bytes.extend_from_slice(&(44_u64 + extension.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&entries_on_disk.to_le_bytes());
    bytes.extend_from_slice(&entries_total.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(extension);
    let record_offset = 0_u64;
    let locator_offset = bytes.len() as u64;
    bytes.extend_from_slice(b"PK\x06\x07");
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&record_offset.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(b"PK\x05\x06");
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    assert_eq!(locator_offset + 20, (bytes.len() - 22) as u64);
    bytes
}

fn zip64_one_entry(root: &Path) -> Vec<u8> {
    let base_path = root.join("zip64-base.zip");
    zip_file(&base_path, &[("tool", b"ok", 0o644)]);
    let base = fs::read(base_path).expect("base ZIP");
    let end = eocd(&base);
    let central_size = u32::from_le_bytes([
        base[end + 12],
        base[end + 13],
        base[end + 14],
        base[end + 15],
    ]);
    let central_offset = u32::from_le_bytes([
        base[end + 16],
        base[end + 17],
        base[end + 18],
        base[end + 19],
    ]);
    let record_offset = end as u64;
    let mut bytes = base[..end].to_vec();
    bytes.extend_from_slice(b"PK\x06\x06");
    bytes.extend_from_slice(&44_u64.to_le_bytes());
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&u64::from(central_size).to_le_bytes());
    bytes.extend_from_slice(&u64::from(central_offset).to_le_bytes());
    bytes.extend_from_slice(b"PK\x06\x07");
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&record_offset.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(b"PK\x05\x06");
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes
}

#[test]
fn zip_eocd_comment_without_trailing_data_is_admitted() {
    let temp = scratch("zip-comment");
    let root = scratch_root(&temp);
    let base = root.join("base.zip");
    zip_file(&base, &[("tool", b"ok", 0o644)]);
    let mut bytes = fs::read(base).expect("ZIP bytes");
    let end = eocd(&bytes);
    bytes[end + 20..end + 22].copy_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(b"note");
    let archive = write_zip(&root, &bytes, "comment.zip");
    extract(
        &archive,
        &root.join("comment-prefix"),
        "https://example.test/tool.zip",
    )
    .expect("valid comment remains supported");
}

#[test]
fn zip_trailing_bytes_and_malformed_comment_eocd_follow_reader_fallback() {
    let temp = scratch("zip-tail");
    let root = scratch_root(&temp);
    let base = root.join("base.zip");
    zip_file(&base, &[("tool", b"ok", 0o644)]);
    let bytes = fs::read(base).expect("ZIP bytes");
    let mut trailing = bytes.clone();
    trailing.extend_from_slice(b"tail");
    assert!(zip::ZipArchive::new(Cursor::new(trailing.clone())).is_ok());
    let trailing_archive = write_zip(&root, &trailing, "trailing.zip");
    extract(
        &trailing_archive,
        &root.join("trailing-prefix"),
        "https://example.test/tool.zip",
    )
    .expect("the selected bounded EOCD can have trailing bytes");

    let mut malformed_comment = bytes;
    let end = eocd(&malformed_comment);
    malformed_comment[end + 20..end + 22].copy_from_slice(&6_u16.to_le_bytes());
    malformed_comment.extend_from_slice(b"PK\x05\x06xx");
    assert!(zip::ZipArchive::new(Cursor::new(malformed_comment.clone())).is_ok());
    let malformed_archive = write_zip(&root, &malformed_comment, "malformed-comment.zip");
    extract(
        &malformed_archive,
        &root.join("malformed-prefix"),
        "https://example.test/tool.zip",
    )
    .expect("a truncated marker in the comment is skipped like the pinned reader");
}

#[test]
fn zip_failed_latest_central_directory_falls_back_with_both_candidates_bounded() {
    let temp = scratch("zip-fallback");
    let root = scratch_root(&temp);
    let base = root.join("base.zip");
    zip_file(&base, &[("tool", b"ok", 0o644)]);
    let mut bytes = fs::read(base).expect("ZIP bytes");
    let end = eocd(&bytes);
    bytes[end + 20..end + 22].copy_from_slice(&22_u16.to_le_bytes());
    bytes.extend_from_slice(b"PK\x05\x06");
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    assert!(zip::ZipArchive::new(Cursor::new(bytes.clone())).is_ok());
    let archive = write_zip(&root, &bytes, "fallback.zip");
    extract(
        &archive,
        &root.join("fallback-prefix"),
        "https://example.test/tool.zip",
    )
    .expect("the failed latest central directory falls back to its earlier EOCD");
}

#[test]
fn zip64_fallback_directory_over_limit_is_rejected_before_parser_allocation() {
    let temp = scratch("zip64-fallback");
    let root = scratch_root(&temp);
    let base_path = root.join("base.zip");
    zip_file(&base_path, &[("tool", b"ok", 0o644)]);
    let base = fs::read(base_path).expect("ZIP bytes");
    let original_end = eocd(&base);
    let central_offset = u32::from_le_bytes([
        base[original_end + 16],
        base[original_end + 17],
        base[original_end + 18],
        base[original_end + 19],
    ]);
    let central_size = u32::from_le_bytes([
        base[original_end + 12],
        base[original_end + 13],
        base[original_end + 14],
        base[original_end + 15],
    ]);
    let prefix = vec![0_u8; 6 * 1024 * 1024];
    let mut bytes = prefix;
    bytes.extend_from_slice(&base);
    let end = bytes.len() - base.len() + original_end;
    let extension_length: u16 = 56 + 20 + 22;
    bytes[end + 20..end + 22].copy_from_slice(&extension_length.to_le_bytes());
    bytes.extend_from_slice(b"PK\x06\x06");
    bytes.extend_from_slice(&44_u64.to_le_bytes());
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&45_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let count = MAX_ENTRIES as u64 + 1;
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&u64::from(central_size).to_le_bytes());
    bytes.extend_from_slice(&u64::from(central_offset).to_le_bytes());
    bytes.extend_from_slice(b"PK\x06\x07");
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&(base.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(b"PK\x05\x06");
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    let archive = write_zip(&root, &bytes, "too-many-fallback.zip");
    let mut source = fs::File::open(&archive).expect("archive source");
    let error = super::super::super::zip_checks::preflight_zip_entries(
        &mut source,
        CheckDeadline::after(Duration::from_secs(60)).expect("deadline"),
    )
    .expect_err("the fallback directory is over the admitted entry count");
    assert!(matches!(
        error,
        OrchestratorError::Internal { problem } if problem == "tool_archive_entry_limit"
    ));
    let destination = root.join("too-many-prefix");
    assert!(extract(&archive, &destination, "https://example.test/tool.zip").is_err());
    assert!(!destination.exists());
}

#[test]
fn zip32_count_disagreement_and_multidisk_are_rejected() {
    let temp = scratch("zip32-fields");
    let root = scratch_root(&temp);
    let base = root.join("base.zip");
    zip_file(&base, &[("one", b"1", 0o644), ("two", b"2", 0o644)]);
    let bytes = fs::read(base).expect("ZIP bytes");
    let end = eocd(&bytes);

    let mut mismatch = bytes.clone();
    mismatch[end + 8..end + 10].copy_from_slice(&1_u16.to_le_bytes());
    assert!(zip::ZipArchive::new(Cursor::new(mismatch.clone())).is_ok());
    assert_rejected(&root, &mismatch, "count-mismatch.zip");

    let mut multidisk = bytes;
    multidisk[end + 4..end + 6].copy_from_slice(&1_u16.to_le_bytes());
    assert_rejected(&root, &multidisk, "multidisk.zip");
}

#[test]
fn zip64_entry_count_and_multidisk_are_bounded_before_parser_allocation() {
    let temp = scratch("zip64-fields");
    let root = scratch_root(&temp);
    let valid = empty_zip64(0, 0, &[]);
    let archive = write_zip(&root, &valid, "empty-zip64.zip");
    extract(
        &archive,
        &root.join("zip64-prefix"),
        "https://example.test/tool.zip",
    )
    .expect("valid small ZIP64 archive");

    let valid_entry = zip64_one_entry(&root);
    let valid_entry_archive = write_zip(&root, &valid_entry, "one-entry-zip64.zip");
    extract(
        &valid_entry_archive,
        &root.join("one-entry-zip64-prefix"),
        "https://example.test/tool.zip",
    )
    .expect("valid ZIP64 central-directory fields");

    let mut mismatch = valid_entry;
    let zip64_end = mismatch
        .windows(4)
        .position(|window| window == b"PK\x06\x06")
        .expect("ZIP64 end record");
    mismatch[zip64_end + 24..zip64_end + 32].copy_from_slice(&0_u64.to_le_bytes());
    assert_rejected(&root, &mismatch, "zip64-count-mismatch.zip");

    let mut multidisk = valid;
    let locator = multidisk
        .windows(4)
        .position(|window| window == b"PK\x06\x07")
        .expect("ZIP64 locator");
    multidisk[locator + 4..locator + 8].copy_from_slice(&1_u32.to_le_bytes());
    assert_rejected(&root, &multidisk, "zip64-multidisk.zip");
}

#[test]
fn zip64_extensible_data_has_a_fixed_admission_bound() {
    let temp = scratch("zip64-extension");
    let root = scratch_root(&temp);
    let extension = vec![0_u8; 64 * 1024 + 1];
    let bytes = empty_zip64(0, 0, &extension);
    assert_rejected(&root, &bytes, "large-zip64-extension.zip");
}

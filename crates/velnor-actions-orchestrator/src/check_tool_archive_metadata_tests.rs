//! Regression tests for bounded GNU and PAX tar extension metadata.

use super::*;

#[test]
fn oversized_gnu_longname_metadata_is_rejected_before_tar_entry_parsing() {
    assert_oversized_tar_extension_is_rejected(EntryType::GNULongName);
}

#[test]
fn oversized_pax_metadata_is_rejected_before_tar_entry_parsing() {
    assert_oversized_tar_extension_is_rejected(EntryType::XHeader);
}

#[test]
fn rust_toolchain_extension_count_within_bound_is_admitted() {
    use std::time::Duration;

    let temp = scratch("extension-count-admitted");
    let root = scratch_root(&temp);
    let archive = root.join("metadata.tar.gz");
    tar_gz_extension_entries(&archive, 65);
    let deadline = CheckDeadline::after(Duration::from_secs(300)).expect("deadline");
    let file = fs::File::open(archive).expect("archive");
    let extensions = tar_preflight::preflight_tar(gzip_reader(file, deadline))
        .expect("bounded extension count is admitted");
    assert_eq!(extensions, 65);
}

#[test]
fn tar_extension_count_above_bound_is_rejected_separately_from_size() {
    let temp = scratch("extension-count-limit");
    let root = scratch_root(&temp);
    let archive = root.join("metadata.tar.gz");
    tar_gz_extension_entries(&archive, tar_preflight::MAX_TAR_EXTENSION_ENTRIES + 1);
    let file = fs::File::open(archive).expect("archive");
    let deadline = CheckDeadline::after(std::time::Duration::from_secs(60)).expect("deadline");
    let error = tar_preflight::preflight_tar(gzip_reader(file, deadline))
        .expect_err("excessive extension entry count is rejected");
    assert!(
        matches!(&error, OrchestratorError::Internal { problem } if problem == "tool_archive_metadata_entry_limit"),
        "unexpected error: {error}"
    );
}

fn assert_oversized_tar_extension_is_rejected(kind: EntryType) {
    let temp = scratch("extension-limit");
    let root = scratch_root(&temp);
    let archive = root.join("metadata.tar.gz");
    tar_gz_extension_with_declared_size(
        &archive,
        kind,
        tar_preflight::MAX_TAR_EXTENSION_ENTRY_BYTES + 1,
    );
    let destination = root.join("prefix");
    let error = extract(&archive, &destination, "https://example.test/tool.tgz")
        .expect_err("oversized extension metadata is rejected");
    assert!(
        matches!(&error, OrchestratorError::Internal { problem } if problem == "tool_archive_metadata_entry_size_limit"),
        "unexpected error: {error}"
    );
    assert!(!destination.exists());
}

fn tar_gz_extension_entries(path: &Path, count: usize) {
    let file = fs::File::create(path).expect("archive");
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(encoder);
    for index in 0..count {
        let mut header = Header::new_gnu();
        header
            .set_path(format!("metadata-{index}"))
            .expect("metadata path");
        header.set_entry_type(EntryType::GNULongName);
        header.set_mode(0o644);
        header.set_size(8);
        header.set_cksum();
        builder
            .append(&header, Cursor::new(b"name\0\0\0\0"))
            .expect("metadata entry");
    }
    builder
        .into_inner()
        .expect("tar finish")
        .finish()
        .expect("gzip finish");
}

#[path = "check_tool_archive_zip_admission_tests.rs"]
mod zip_admission_tests;

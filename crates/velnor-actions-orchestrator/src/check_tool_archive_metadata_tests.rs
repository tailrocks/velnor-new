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
fn per_file_pax_metadata_above_256_entries_extracts_within_existing_limits() {
    const PAX_FILES: usize = 5_132;
    let temp = scratch("pax-extension-count-admitted");
    let root = scratch_root(&temp);
    let archive = root.join("metadata.tar.gz");
    tar_gz_pax_file_entries(&archive, PAX_FILES, 128);
    let file = fs::File::open(&archive).expect("archive");
    let deadline = CheckDeadline::after(std::time::Duration::from_secs(300)).expect("deadline");
    let extensions = tar_preflight::preflight_tar(gzip_reader(file, deadline))
        .expect("The test could not validate PAX metadata within the byte and total entry limits.");
    assert_eq!(extensions, PAX_FILES);

    let destination = root.join("prefix");
    extract(&archive, &destination, "https://example.test/tool.tar.gz")
        .expect("The test could not extract the PAX archive.");
    assert_eq!(
        fs::read_dir(&destination)
            .expect("The test could not read the destination entries.")
            .count(),
        PAX_FILES
    );
    assert_eq!(
        fs::read(destination.join("member-00000"))
            .expect("The test could not read the first archive member."),
        b"x"
    );
    assert_eq!(
        fs::read(destination.join(format!("member-{:05}", PAX_FILES - 1)))
            .expect("The test could not read the last archive member."),
        b"x"
    );
}

#[test]
fn metadata_headers_are_charged_to_the_shared_archive_entry_budget() {
    let temp = scratch("extension-global-budget");
    let root = scratch_root(&temp);
    let archive = root.join("metadata.tar.gz");
    tar_gz_pax_file_entries(&archive, 5, 8);
    let destination = root.join("prefix");
    let mut budget = ArchiveBudget {
        entries: 4,
        ..ArchiveBudget::new()
    };
    let deadline = CheckDeadline::after(std::time::Duration::from_secs(60)).expect("deadline");
    let error = extract_archive(
        &archive,
        &destination,
        "https://example.test/tool.tar.gz",
        &mut budget,
        deadline,
    )
    .expect_err("The extractor must charge extension headers to the shared entry budget.");
    assert!(
        matches!(&error, OrchestratorError::Internal { problem } if problem == "tool_archive_global_entry_limit"),
        "unexpected error: {error}"
    );
    assert!(!destination.exists());
}

#[test]
fn aggregate_pax_metadata_limit_is_enforced_without_destination_creation() {
    const EXTENSIONS: usize = 18;
    const VALUE_BYTES: usize = 60 * 1024;
    let temp = scratch("extension-total-bytes");
    let root = scratch_root(&temp);
    let archive = root.join("metadata.tar.gz");
    tar_gz_pax_file_entries(&archive, EXTENSIONS, VALUE_BYTES);
    let destination = root.join("prefix");
    let error = extract(&archive, &destination, "https://example.test/tool.tar.gz")
        .expect_err("The extractor must enforce the total PAX metadata limit.");
    assert!(
        matches!(&error, OrchestratorError::Internal { problem } if problem == "tool_archive_metadata_size_limit"),
        "unexpected error: {error}"
    );
    assert!(!destination.exists());
}

#[test]
fn total_tar_header_limit_still_rejects_excess_extension_entries() {
    let temp = scratch("extension-total-entries");
    let root = scratch_root(&temp);
    let archive = root.join("metadata.tar.gz");
    tar_gz_extension_entries(&archive, MAX_ENTRIES + 1);
    let file = fs::File::open(archive).expect("archive");
    let deadline = CheckDeadline::after(std::time::Duration::from_secs(300)).expect("deadline");
    let error = tar_preflight::preflight_tar(gzip_reader(file, deadline))
        .expect_err("The preflight check must enforce the total tar header limit.");
    assert!(
        matches!(&error, OrchestratorError::Internal { problem } if problem == "tool_archive_entry_limit"),
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

fn tar_gz_pax_file_entries(path: &Path, count: usize, comment_bytes: usize) {
    let file = fs::File::create(path).expect("archive");
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(encoder);
    let comment = vec![b'x'; comment_bytes];
    for index in 0..count {
        builder
            .append_pax_extensions([("comment", comment.as_slice())])
            .expect("The test could not append PAX metadata.");
        let mut header = Header::new_gnu();
        header
            .set_path(format!("member-{index:05}"))
            .expect("The test could not set the archive member path.");
        header.set_entry_type(EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(1);
        header.set_cksum();
        builder
            .append(&header, Cursor::new(b"x"))
            .expect("The test could not append the archive member.");
    }
    builder
        .into_inner()
        .expect("tar finish")
        .finish()
        .expect("gzip finish");
}

#[path = "check_tool_archive_zip_admission_tests.rs"]
mod zip_admission_tests;

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

#[path = "check_tool_archive_zip_admission_tests.rs"]
mod zip_admission_tests;

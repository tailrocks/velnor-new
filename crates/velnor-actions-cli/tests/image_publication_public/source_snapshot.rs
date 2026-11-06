// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::support::Fixture;
use std::{error::Error, fs};

#[test]
fn rejects_untrusted_commit_snapshots_before_registry_mutation() -> Result<(), Box<dyn Error>> {
    for (fault, expected) in [
        ("tree_truncated", "image_publication_source_tree"),
        ("tree_missing", "image_publication_source_inventory"),
        ("tree_nonregular", "image_publication_source_tree"),
        ("tree_duplicate", "image_publication_source_tree"),
        (
            "tree_blob_mismatch",
            "image_publication_source_archive_digest",
        ),
        ("archive_missing", "image_publication_source_inventory"),
        ("archive_duplicate", "image_publication_source_archive"),
        ("archive_altered", "image_publication_source_archive_digest"),
        ("archive_symlink", "image_publication_source_archive"),
        ("archive_mode", "image_publication_source_archive_mode"),
        ("other_prefix", "image_publication_source_archive"),
        ("traversal", "image_publication_source_archive"),
        ("file_cap", "image_publication_source_archive"),
        ("compressed_cap", "image_publication_source_archive"),
        ("expanded_cap", "image_publication_source_archive"),
    ] {
        let fixture = Fixture::new()?;
        fixture.set_snapshot_fault(fault)?;
        let registry_before = fixture.registry_state()?;
        let archive_before = fs::read(&fixture.archive)?;
        let record_before = fs::read(&fixture.record)?;
        let output = fixture.invoke()?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "accepted {fault}");
        assert!(
            stderr.contains(expected),
            "{fault}: expected {expected}; got {stderr}"
        );
        assert!(!fixture.receipt.exists(), "created receipt for {fault}");
        assert_eq!(
            fixture.registry_state()?,
            registry_before,
            "registry changed for {fault}"
        );
        assert_eq!(fs::read(&fixture.archive)?, archive_before);
        assert_eq!(fs::read(&fixture.record)?, record_before);
    }
    Ok(())
}

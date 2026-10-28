// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use super::support::{CapturedOutcome, ExistingTag, Fixture, ReplayMutation};

#[test]
fn public_job_publication_accepts_legacy_raw_tar_layer() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new_with_raw_tar()?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let manifest_path = blob_path(&fixture.expected_digest);
    let manifest: serde_json::Value = serde_json::from_slice(
        fixture
            .expected_inventory
            .get(&manifest_path)
            .ok_or("raw-tar manifest is missing")?,
    )?;
    assert_eq!(
        manifest["layers"][0]["mediaType"],
        "application/vnd.oci.image.layer.v1.tar"
    );
    let output = fixture.invoke()?;
    assert!(
        output.status.success(),
        "raw-tar publication failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.receipt)?)?;
    assert_eq!(receipt["idempotent"], false);
    assert_eq!(
        fixture.registry_digest()?,
        Some(fixture.expected_digest.clone())
    );
    assert_eq!(fixture.stored_inventory()?, fixture.expected_inventory);
    assert_eq!(fs::read(&fixture.archive)?, archive_before);
    assert_eq!(fs::read(&fixture.record)?, record_before);
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 0);
    assert!(fixture.last_fixture_error()?.is_none());
    Ok(())
}

#[test]
fn public_job_publication_is_idempotent_for_the_same_registry_manifest()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.seed_existing_tag(ExistingTag::SameDigest)?;
    let registry_before = fixture.registry_state()?;
    let output = fixture.invoke()?;
    assert!(
        output.status.success(),
        "idempotent publication failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.receipt)?)?;
    assert_eq!(receipt["idempotent"], true);
    assert_eq!(fixture.registry_state()?, registry_before);
    assert_eq!(
        fixture.registry_digest()?,
        Some(fixture.expected_digest.clone())
    );
    assert_eq!(fixture.stored_inventory()?, fixture.expected_inventory);
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 0);
    assert!(fixture.last_fixture_error()?.is_none());
    Ok(())
}

#[test]
fn public_job_publication_rejects_conflicting_leaf_and_outer_index_tags()
-> Result<(), Box<dyn Error>> {
    for tag in [ExistingTag::ConflictingLeaf, ExistingTag::OuterIndex] {
        let fixture = Fixture::new()?;
        fixture.seed_existing_tag(tag)?;
        let registry_before = fixture.registry_state()?;
        let archive_before = fs::read(&fixture.archive)?;
        let record_before = fs::read(&fixture.record)?;
        let output = fixture.invoke()?;
        assert!(
            !output.status.success(),
            "accepted conflicting registry tag"
        );
        let fixture_error = fixture.last_fixture_error()?;
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("image_publication_tag_conflict"),
            "unexpected tag conflict error: {}; fixture: {fixture_error:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(fixture.last_fixture_error()?.is_none());
        assert!(!fixture.receipt.exists());
        assert_eq!(fixture.registry_state()?, registry_before);
        assert_eq!(fs::read(&fixture.archive)?, archive_before);
        assert_eq!(fs::read(&fixture.record)?, record_before);
        assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 0);
    }
    Ok(())
}

#[test]
fn public_job_publication_requires_the_captured_missing_manifest_response()
-> Result<(), Box<dyn Error>> {
    for (outcome, mutation) in [
        (CapturedOutcome::Unauthorized, ReplayMutation::Exact),
        (CapturedOutcome::Transport, ReplayMutation::Exact),
    ] {
        let fixture = Fixture::new()?;
        fixture.set_initial_manifest_outcome(outcome, mutation)?;
        assert_rejects_initial_tag_response(&fixture)?;
    }
    for mutation in [
        ReplayMutation::WrongExit,
        ReplayMutation::NonemptyStdout,
        ReplayMutation::WrongReference,
        ReplayMutation::WrongUrl,
        ReplayMutation::ExtraLine,
    ] {
        let fixture = Fixture::new()?;
        fixture.set_initial_manifest_outcome(CapturedOutcome::Missing, mutation)?;
        assert_rejects_initial_tag_response(&fixture)?;
    }
    Ok(())
}

#[test]
fn public_job_publication_rejects_a_tag_that_moves_after_readback() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let moved = fixture.move_tag_after_readback()?;
    let foreign_state = fixture.preseed_registry_state()?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let sentinel = fixture.scratch.join("foreign-sentinel");
    fs::write(&sentinel, b"preserve external scratch")?;
    let output = fixture.invoke()?;
    assert!(
        !output.status.success(),
        "accepted a tag that moved after readback"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("image_publication_tag_changed"),
        "unexpected tag movement error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.last_fixture_error()?.is_none());
    assert!(!fixture.receipt.exists());
    assert_eq!(fixture.registry_digest()?, Some(moved.digest));
    let stored = fixture.stored_inventory()?;
    for (path, bytes) in moved.inventory {
        assert_eq!(stored.get(&path), Some(&bytes));
    }
    let registry_after = fixture.registry_state()?;
    assert_ne!(registry_after, foreign_state);
    assert_eq!(
        registry_after["foreign_tags"],
        foreign_state["foreign_tags"]
    );
    assert_eq!(
        registry_after["foreign_inventory"],
        foreign_state["foreign_inventory"]
    );
    assert_eq!(fs::read(&fixture.archive)?, archive_before);
    assert_eq!(fs::read(&fixture.record)?, record_before);
    assert_eq!(fs::read(&sentinel)?, b"preserve external scratch");
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
    Ok(())
}

#[test]
fn public_job_publication_rejects_a_same_size_corrupt_remote_manifest() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new()?;
    let foreign_state = fixture.preseed_registry_state()?;
    let manifest_path = blob_path(&fixture.expected_digest);
    let manifest = fixture
        .expected_inventory
        .get(&manifest_path)
        .ok_or("expected manifest is missing")?;
    let marker = b"\"schemaVersion\":2";
    let marker_offset = manifest
        .windows(marker.len())
        .position(|window| window == marker)
        .ok_or("expected schema marker is missing")?;
    let offset = u64::try_from(marker_offset + b"\"schemaVersion\":".len())?;
    fixture.corrupt_remote_manifest_response_at(offset)?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let sentinel = fixture.scratch.join("foreign-sentinel");
    fs::write(&sentinel, b"preserve external scratch")?;
    let output = fixture.invoke()?;
    assert!(
        !output.status.success(),
        "accepted a corrupt immutable manifest"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("image_publication_remote_manifest"),
        "unexpected immutable manifest error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.last_fixture_error()?.is_none());
    assert!(!fixture.receipt.exists());
    assert_eq!(
        fixture.registry_digest()?,
        Some(fixture.expected_digest.clone())
    );
    assert_eq!(fixture.stored_inventory()?, fixture.expected_inventory);
    let registry_after = fixture.registry_state()?;
    assert_ne!(registry_after, foreign_state);
    assert_eq!(
        registry_after["foreign_tags"],
        foreign_state["foreign_tags"]
    );
    assert_eq!(
        registry_after["foreign_inventory"],
        foreign_state["foreign_inventory"]
    );
    assert_eq!(fs::read(&fixture.archive)?, archive_before);
    assert_eq!(fs::read(&fixture.record)?, record_before);
    assert_eq!(fs::read(&sentinel)?, b"preserve external scratch");
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
    Ok(())
}

fn assert_rejects_initial_tag_response(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let registry_before = fixture.registry_state()?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let output = fixture.invoke()?;
    assert!(
        !output.status.success(),
        "accepted a non-exact missing-tag response"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("image_publication_tag_lookup"),
        "unexpected initial tag lookup error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.last_fixture_error()?.is_none());
    assert!(!fixture.receipt.exists());
    assert_eq!(fixture.registry_state()?, registry_before);
    assert_eq!(fs::read(&fixture.archive)?, archive_before);
    assert_eq!(fs::read(&fixture.record)?, record_before);
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 0);
    Ok(())
}

fn blob_path(digest: &str) -> String {
    format!("blobs/sha256/{}", digest.trim_start_matches("sha256:"))
}

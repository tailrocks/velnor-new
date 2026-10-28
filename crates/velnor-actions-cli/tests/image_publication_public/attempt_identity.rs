// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use super::support::{AttemptArtifactCase as Case, Fixture, FixtureProfile};

#[test]
fn public_runner_publication_selects_only_the_current_attempt_artifact()
-> Result<(), Box<dyn Error>> {
    let mut fixture = Fixture::new_for_attempt(FixtureProfile::Runner, 2)?;
    fixture.configure_attempt_two_artifacts(Case::OlderAndCurrent)?;
    let expected_artifacts = vec![fixture.artifact_id - 1, fixture.artifact_id];
    assert_eq!(fixture.artifact_ids()?, expected_artifacts);

    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let output = fixture.invoke()?;
    let fixture_error = fixture.last_fixture_error()?;
    assert!(
        output.status.success(),
        "attempt-two publication failed: {}; fixture: {fixture_error:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture_error.is_none());
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.receipt)?)?;
    assert_eq!(receipt["producer_run_attempt"], 2);
    assert_eq!(receipt["artifact_id"], fixture.artifact_id);
    assert_eq!(
        fixture.registry_digest()?,
        Some(fixture.expected_digest.clone())
    );
    assert_eq!(fs::read(&fixture.archive)?, archive_before);
    assert_eq!(fs::read(&fixture.record)?, record_before);
    assert_eq!(fixture.stored_inventory()?, fixture.expected_inventory);
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 0);
    Ok(())
}

#[test]
fn public_runner_publication_rejects_artifacts_from_other_or_ambiguous_attempts()
-> Result<(), Box<dyn Error>> {
    for (case, expected_error) in [
        (Case::OlderOnly, "image_publication_artifact_count"),
        (
            Case::PreviousArtifactId,
            "image_publication_artifact_identity",
        ),
        (
            Case::UnqualifiedCurrentName,
            "image_publication_artifact_count",
        ),
        (
            Case::DuplicateCurrentName,
            "image_publication_artifact_count",
        ),
    ] {
        let mut fixture = Fixture::new_for_attempt(FixtureProfile::Runner, 2)?;
        fixture.configure_attempt_two_artifacts(case)?;
        let foreign = fixture.scratch.join("foreign.keep");
        fs::write(&foreign, b"preserve")?;
        let archive_before = fs::read(&fixture.archive)?;
        let record_before = fs::read(&fixture.record)?;
        let registry_before = fixture.preseed_registry_state()?;

        let output = fixture.invoke()?;
        let fixture_error = fixture.last_fixture_error()?;
        assert!(
            !output.status.success(),
            "accepted attempt artifact rejection {case:?}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected_error),
            "wrong rejection for {case:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            fixture_error.is_none(),
            "fixture failed for {case:?}: {fixture_error:?}"
        );
        assert!(!fixture.receipt.exists(), "receipt for {case:?}");
        assert_eq!(
            fixture.registry_state()?,
            registry_before,
            "registry for {case:?}"
        );
        assert_eq!(
            fs::read(&fixture.archive)?,
            archive_before,
            "archive for {case:?}"
        );
        assert_eq!(
            fs::read(&fixture.record)?,
            record_before,
            "record for {case:?}"
        );
        assert_eq!(
            fs::read(&foreign)?,
            b"preserve",
            "foreign file for {case:?}"
        );
        assert_eq!(
            fs::read_dir(&fixture.scratch)?.count(),
            1,
            "scratch for {case:?}"
        );
    }
    Ok(())
}

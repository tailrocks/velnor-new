// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use super::support;

#[test]
fn public_runner_family_publication_authenticates_selected_source_and_verifies_registry_state()
-> Result<(), Box<dyn Error>> {
    for profile in [
        support::FixtureProfile::Runner,
        support::FixtureProfile::Dind,
        support::FixtureProfile::ResourceProbe,
    ] {
        let fixture = support::Fixture::new_for(profile)?;
        assert_eq!(fixture.source, fixture.authority_sha);
        assert_ne!(fixture.source, fixture.current_main_sha);
        assert_eq!(fixture.current_main_sha, "b".repeat(40));
        assert_ne!(
            fixture.producer_run_id,
            support::FixtureProfile::Job.run_id()
        );
        assert_ne!(
            fixture.artifact_id,
            support::FixtureProfile::Job.artifact_id()
        );
        let archive_before = fs::read(&fixture.archive)?;
        let record_before = fs::read(&fixture.record)?;
        let output = fixture.invoke()?;
        let fixture_error = fixture.last_fixture_error()?;
        assert!(
            output.status.success(),
            "{} publication failed: {}; fixture: {fixture_error:?}",
            profile.name(),
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.receipt)?)?;
        assert_eq!(receipt["schema"], "velnor.image-publication.v1");
        assert_eq!(receipt["profile"], profile.name());
        assert_eq!(receipt["source_commit"], fixture.source);
        assert_eq!(
            receipt["workflow_path"],
            ".github/workflows/product-release.yml"
        );
        assert_eq!(receipt["workflow_authority_sha"], fixture.authority_sha);
        assert_eq!(receipt["producer_run_id"], fixture.producer_run_id);
        assert_eq!(receipt["artifact_id"], fixture.artifact_id);
        assert_eq!(receipt["repository"], profile.repository());
        assert_eq!(receipt["tag"], format!("sha-{}", fixture.source));
        assert_eq!(receipt["registry_digest"], fixture.expected_digest);
        assert_eq!(receipt["platform_manifest_digest"], fixture.platform_digest);
        assert_eq!(receipt["image_configuration_digest"], fixture.config_digest);
        assert_eq!(receipt["lock_sha256"], fixture.lock_sha256);
        assert!(fixture.platform_digest.starts_with("sha256:"));
        assert!(fixture.config_digest.starts_with("sha256:"));
        assert_eq!(receipt["archive_sha256"], fixture.archive_sha256);
        assert_eq!(receipt["idempotent"], false);
        assert!(fixture_error.is_none());
        assert_eq!(fs::read(&fixture.archive)?, archive_before);
        assert_eq!(fs::read(&fixture.record)?, record_before);
        assert_eq!(
            fixture.registry_digest()?,
            Some(fixture.expected_digest.clone())
        );
        assert_eq!(fixture.stored_inventory()?, fixture.expected_inventory);
        assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 0);
    }
    Ok(())
}

#[test]
fn public_runner_family_publication_rejects_producer_head_outside_selected_source()
-> Result<(), Box<dyn Error>> {
    let producer_head = "b".repeat(40);
    let mut accepted_profiles = Vec::new();
    for profile in [
        support::FixtureProfile::Runner,
        support::FixtureProfile::Dind,
        support::FixtureProfile::ResourceProbe,
    ] {
        let fixture = support::Fixture::new_with_producer_head(profile, &producer_head)?;
        assert_eq!(fixture.source, "a".repeat(40));
        assert_eq!(fixture.authority_sha, producer_head);
        assert_eq!(fixture.current_main_sha, producer_head);
        let candidate: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.record)?)?;
        assert_eq!(candidate["source"]["commit"], fixture.source);
        assert_eq!(
            candidate["workflow"]["authority_commit"],
            fixture.authority_sha
        );
        let archive_before = fs::read(&fixture.archive)?;
        let record_before = fs::read(&fixture.record)?;
        let registry_before = fixture.preseed_registry_state()?;
        let foreign = fixture.scratch.join("foreign.keep");
        fs::write(&foreign, b"preserve")?;
        let output = fixture.invoke()?;
        let fixture_error = fixture.last_fixture_error()?;
        assert!(
            fixture_error.is_none(),
            "fixture route failed: {fixture_error:?}"
        );
        if output.status.success() {
            let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.receipt)?)?;
            assert_eq!(receipt["source_commit"], fixture.source);
            assert_eq!(receipt["workflow_authority_sha"], fixture.authority_sha);
            assert_ne!(fixture.registry_state()?, registry_before);
            accepted_profiles.push(profile.name());
            continue;
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("image_publication_producer_source_mismatch"),
            "wrong producer source rejection for {}: {stderr}",
            profile.name()
        );
        assert!(!fixture.receipt.exists());
        assert_eq!(fixture.registry_state()?, registry_before);
        assert_eq!(fs::read(&fixture.archive)?, archive_before);
        assert_eq!(fs::read(&fixture.record)?, record_before);
        assert_eq!(fs::read(&foreign)?, b"preserve");
        assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
    }
    assert!(
        accepted_profiles.is_empty(),
        "published a producer/source mismatch for {accepted_profiles:?}"
    );
    Ok(())
}

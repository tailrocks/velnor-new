// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use sha2::{Digest, Sha256};

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
        verify_qualification_fixture(&fixture, profile)?;
        verify_publication(&fixture, profile)?;
    }
    Ok(())
}

fn verify_qualification_fixture(
    fixture: &support::Fixture,
    profile: support::FixtureProfile,
) -> Result<(), Box<dyn Error>> {
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
    let proof_path = fixture
        .qualification_proof
        .as_ref()
        .ok_or("runner family fixture has no qualification receipt")?;
    let proof_bytes = fs::read(proof_path)?;
    let proof: serde_json::Value = serde_json::from_slice(&proof_bytes)?;
    assert_eq!(proof["source_commit"], fixture.source);
    assert_eq!(proof["source_ref"], "refs/heads/main");
    assert_eq!(proof["runner_lock_sha256"], fixture.lock_sha256);
    assert_eq!(proof["producer_run_id"], fixture.producer_run_id);
    assert_eq!(proof["producer_run_attempt"], fixture.producer_run_attempt);
    assert_eq!(proof["qualifier_run_id"], profile.qualification_run_id());
    assert_eq!(proof["qualifier_run_attempt"], 1);
    assert_eq!(proof["qualifier_job_id"], profile.qualification_job_id());
    assert_ne!(fixture.producer_run_id, profile.qualification_run_id());
    assert_ne!(fixture.producer_run_id, profile.publisher_run_id());
    assert_ne!(profile.qualification_run_id(), profile.publisher_run_id());
    let candidate: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.record)?)?;
    assert_eq!(
        proof["images"][profile.name()]["archive"],
        candidate["archive"]
    );
    assert_eq!(proof["images"][profile.name()]["oci"], candidate["oci"]);
    let proof_subject = support::hex(&Sha256::digest(&proof_bytes));
    let qualification_attestation = fixture.qualification_attestation_output()?;
    let certificate =
        &qualification_attestation[0]["verificationResult"]["signature"]["certificate"];
    assert_eq!(
        certificate["buildSignerURI"],
        "https://github.com/tailrocks/velnor-new/.github/workflows/runner-image-qualification-attest.yml@refs/heads/main"
    );
    let attested_subject =
        &qualification_attestation[0]["verificationResult"]["statement"]["subject"][0];
    assert_eq!(attested_subject["name"], "RUNNER_IMAGE_QUALIFICATION.json");
    assert_eq!(attested_subject["digest"]["sha256"], proof_subject);
    Ok(())
}

fn verify_publication(
    fixture: &support::Fixture,
    profile: support::FixtureProfile,
) -> Result<(), Box<dyn Error>> {
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
    let receipt: serde_json::Value = super::support::read_receipt(&fixture.receipt)?;
    assert_eq!(receipt["schema"], "velnor.image-publication.v1");
    assert_eq!(receipt["profile"], profile.name());
    assert_eq!(receipt["source_commit"], fixture.source);
    assert_eq!(receipt["workflow_path"], ".github/workflows/ci.yml");
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
    Ok(())
}

#[test]
fn public_runner_publication_rejects_dind_kind_archive_before_registry_mutation()
-> Result<(), Box<dyn Error>> {
    let fixture = support::Fixture::new_for_image_kind(support::FixtureProfile::Runner, "dind")?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let registry_before = fixture.preseed_registry_state()?;
    let foreign = fixture.scratch.join("foreign.keep");
    fs::write(&foreign, b"preserve")?;

    let output = fixture.invoke()?;
    assert!(fixture.last_fixture_error()?.is_none());
    if output.status.success() {
        let receipt: serde_json::Value = super::support::read_receipt(&fixture.receipt)?;
        assert_eq!(receipt["profile"], "runner");
        assert_eq!(receipt["source_commit"], fixture.source);
        assert_eq!(
            fixture.registry_digest()?,
            Some(fixture.expected_digest.clone())
        );
        return Err("published a Runner record with a same-source DinD-kind archive".into());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("image_publication_image_kind"),
        "wrong-kind archive rejection: {stderr}"
    );
    assert!(!fixture.receipt.exists());
    assert_eq!(fixture.registry_state()?, registry_before);
    assert_eq!(fs::read(&fixture.archive)?, archive_before);
    assert_eq!(fs::read(&fixture.record)?, record_before);
    assert_eq!(fs::read(&foreign)?, b"preserve");
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
    Ok(())
}

#[test]
fn public_runner_family_publication_rejects_producer_head_outside_selected_source()
-> Result<(), Box<dyn Error>> {
    let scenario = "runner_source::public_runner_family_publication_rejects_producer_head_outside_selected_source";
    if !super::begin_isolated(scenario)? {
        return Ok(());
    }
    let mut accepted_profiles = Vec::new();
    for profile in [
        support::FixtureProfile::Runner,
        support::FixtureProfile::Dind,
        support::FixtureProfile::ResourceProbe,
    ] {
        let fixture = support::Fixture::new_for(profile)?;
        fixture.apply_rejection(support::RejectionCase::WrongProducerHead)?;
        assert_eq!(fixture.source, "a".repeat(40));
        assert_eq!(fixture.authority_sha, fixture.source);
        assert_eq!(fixture.current_main_sha, "b".repeat(40));
        let candidate: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.record)?)?;
        assert_eq!(candidate["source"]["commit"], fixture.source);
        assert_eq!(candidate["workflow"]["authority_commit"], fixture.source);
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
            let receipt: serde_json::Value = super::support::read_receipt(&fixture.receipt)?;
            assert_eq!(receipt["source_commit"], fixture.source);
            assert_eq!(receipt["workflow_authority_sha"], fixture.authority_sha);
            assert_ne!(fixture.registry_state()?, registry_before);
            accepted_profiles.push(profile.name());
            continue;
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("image_publication_producer_run_identity"),
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
    super::isolated_worker::record_success("image_publication", scenario)?;
    Ok(())
}

#[test]
fn runner_publication_rejects_lock_pins_inconsistent_with_image_source()
-> Result<(), Box<dyn Error>> {
    let scenario =
        "runner_source::runner_publication_rejects_lock_pins_inconsistent_with_image_source";
    if !super::begin_isolated(scenario)? {
        return Ok(());
    }
    let original = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.velnor/runner.lock"
    ))?;
    for (key, value, expected) in [
        (
            "base_image",
            "ubuntu:26.04@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "image_publication_lock_base",
        ),
        ("runner_version", "2.1.0", "image_publication_lock_argument"),
        (
            "docker_sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "image_publication_lock_argument",
        ),
        (
            "buildx_asset_url",
            "https://example.invalid/buildx",
            "image_publication_lock_url",
        ),
    ] {
        let fixture = support::Fixture::new_for(support::FixtureProfile::Runner)?;
        let before = fixture.preseed_registry_state()?;
        let mut lock: toml::Value = toml::from_str(&original)?;
        lock[key] = toml::Value::String(value.to_owned());
        fixture.replace_authenticated_runner_lock(toml::to_string(&lock)?.as_bytes())?;
        let output = fixture.invoke()?;
        assert!(!output.status.success(), "accepted mismatched {key}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "unexpected rejection for {key}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!fixture.receipt.exists());
        assert_eq!(fixture.registry_state()?, before);
    }
    super::isolated_worker::record_success("image_publication", scenario)?;
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

#[path = "image_publication_public/attempt_identity.rs"]
mod attempt_identity;
#[path = "image_publication_public/registry_cases.rs"]
mod registry_cases;
#[path = "image_publication_public/source_closure.rs"]
mod source_closure;
#[path = "image_publication_public/support/mod.rs"]
mod support;

#[test]
fn public_job_publication_accepts_captured_missing_manifest_and_verifies_registry_state()
-> Result<(), Box<dyn Error>> {
    let fixture = support::Fixture::new()?;
    fixture.set_initial_manifest_outcome(
        support::CapturedOutcome::Missing,
        support::ReplayMutation::Exact,
    )?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let output = fixture.invoke()?;
    let fixture_error = fixture.last_fixture_error()?;
    assert!(
        output.status.success(),
        "publication failed: {}; fixture: {fixture_error:?}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(fixture_error.is_none());
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.receipt)?)?;
    assert_eq!(receipt["schema"], "velnor.image-publication.v1");
    assert_eq!(receipt["profile"], "job");
    assert_eq!(receipt["source_commit"], fixture.source);
    assert_eq!(receipt["workflow_path"], fixture.profile.workflow_path());
    assert_eq!(receipt["workflow_authority_sha"], fixture.authority_sha);
    assert_eq!(receipt["repository"], fixture.profile.repository());
    assert_eq!(receipt["tag"], format!("sha-{}", fixture.source));
    assert_eq!(receipt["registry_digest"], fixture.expected_digest);
    assert_eq!(receipt["platform_manifest_digest"], fixture.platform_digest);
    assert_eq!(receipt["image_configuration_digest"], fixture.config_digest);
    assert_eq!(receipt["lock_sha256"], fixture.lock_sha256);
    assert_eq!(receipt["archive_sha256"], fixture.archive_sha256);
    assert_eq!(receipt["producer_run_id"], fixture.producer_run_id);
    assert_eq!(receipt["artifact_id"], fixture.artifact_id);
    assert!(fixture.platform_digest.starts_with("sha256:"));
    assert!(fixture.config_digest.starts_with("sha256:"));
    assert_eq!(receipt["idempotent"], false);
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
fn public_runner_family_publication_authenticates_selected_source_and_verifies_registry_state()
-> Result<(), Box<dyn Error>> {
    for profile in [
        support::FixtureProfile::Runner,
        support::FixtureProfile::Dind,
        support::FixtureProfile::ResourceProbe,
    ] {
        let fixture = support::Fixture::new_for(profile)?;
        assert_ne!(fixture.source, fixture.authority_sha);
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
fn public_authority_rejections_preserve_registry_inputs_and_foreign_resources()
-> Result<(), Box<dyn Error>> {
    use support::RejectionCase as Case;

    for case in [
        Case::WrongRepository,
        Case::WrongSelectedSource,
        Case::WrongCallerWorkflow,
        Case::WrongReusableWorkflow,
        Case::WrongRunAttempt,
        Case::WrongArtifact,
        Case::FailedLatestCi,
        Case::StaleLatestCi,
        Case::WrongAttestationDigest,
        Case::MissingAttestationSubject,
        Case::WrongRunnerLock,
        Case::FailedQualification,
        Case::ArchiveSubstitution,
    ] {
        let fixture = support::Fixture::new_for(case.profile())?;
        fixture.apply_rejection(case)?;
        let foreign = fixture.scratch.join("foreign.keep");
        fs::write(&foreign, b"preserve")?;
        let archive_before = fs::read(&fixture.archive)?;
        let record_before = fs::read(&fixture.record)?;
        let registry_before = fixture.preseed_registry_state()?;
        let output = fixture.invoke()?;
        let fixture_error = fixture.last_fixture_error()?;
        assert!(
            !output.status.success(),
            "accepted authority rejection {case:?}"
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
        if matches!(case, Case::WrongAttestationDigest) {
            assert!(
                fixture_error
                    .as_deref()
                    .is_some_and(|error| error.contains("unattested subject")),
                "fixture did not reject the requested attestation digest: {fixture_error:?}"
            );
        } else {
            assert!(
                fixture_error.is_none(),
                "fixture route failed for {case:?}: {fixture_error:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn public_job_publication_keeps_legacy_optional_config_acceptance() -> Result<(), Box<dyn Error>> {
    for config in [
        serde_json::json!({"User":"fixture"}),
        serde_json::json!({"Cmd":17}),
    ] {
        let fixture = support::Fixture::new_with_image_config(Some(config))?;
        let archive_before = fs::read(&fixture.archive)?;
        let record_before = fs::read(&fixture.record)?;
        let output = fixture.invoke()?;
        let fixture_error = fixture.last_fixture_error()?;
        assert!(
            output.status.success(),
            "legacy Job config rejected: {}; fixture: {fixture_error:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(fixture.receipt.is_file());
        assert!(fixture_error.is_none());
        assert_eq!(fixture.stored_inventory()?, fixture.expected_inventory);
        assert_eq!(fs::read(&fixture.archive)?, archive_before);
        assert_eq!(fs::read(&fixture.record)?, record_before);
        assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 0);
    }
    Ok(())
}

#[test]
fn public_job_publication_failure_cleans_owned_scratch_and_preserves_inputs()
-> Result<(), Box<dyn Error>> {
    let fixture = support::Fixture::new()?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let foreign = fixture.scratch.join("foreign.keep");
    fs::write(&foreign, b"preserve")?;
    fixture.fail_remote_readback()?;
    let output = fixture.invoke()?;
    assert!(!output.status.success());
    assert!(fixture.last_fixture_error()?.is_none());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("image_publication_blob_process"),
        "unexpected publication failure: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fixture.receipt.exists());
    assert_eq!(fs::read(&foreign)?, b"preserve");
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
    assert_eq!(fs::read(&fixture.archive)?, archive_before);
    assert_eq!(fs::read(&fixture.record)?, record_before);
    Ok(())
}

#[test]
fn public_job_publication_rejects_corrupt_remote_config_and_layer() -> Result<(), Box<dyn Error>> {
    for kind in ["config", "layer"] {
        for fault in blob_faults(kind)? {
            assert_blob_fault_rejected(kind, fault)?;
        }
    }
    Ok(())
}

fn blob_faults(kind: &str) -> Result<Vec<support::BlobFault>, Box<dyn Error>> {
    use support::BlobFault as Fault;

    let fixture = support::Fixture::new_with_large_raw_tar()?;
    let digest = match kind {
        "config" => &fixture.config_digest,
        "layer" => fixture
            .layer_digests
            .first()
            .ok_or("large layer digest is missing")?,
        _ => return Err("unsupported blob kind".into()),
    };
    let path = blob_path(digest);
    let size = fixture
        .expected_inventory
        .get(&path)
        .ok_or("readback fixture blob is missing")?
        .len();
    let last = u64::try_from(
        size.checked_sub(1)
            .ok_or("readback fixture blob is empty")?,
    )?;
    let mut faults = vec![
        Fault::Missing,
        Fault::Short,
        Fault::Oversize,
        Fault::Nonzero,
    ];
    if kind == "config" {
        faults.extend([Fault::CorruptAt(0), Fault::CorruptAt(last)]);
    } else {
        assert!(size > 32 * 1024);
        faults.extend([Fault::CorruptAt(32 * 1024 + 7), Fault::CorruptAt(last)]);
    }
    Ok(faults)
}

fn assert_blob_fault_rejected(kind: &str, fault: support::BlobFault) -> Result<(), Box<dyn Error>> {
    use support::BlobFault as Fault;

    let fixture = support::Fixture::new_with_large_raw_tar()?;
    let digest = match kind {
        "config" => &fixture.config_digest,
        "layer" => fixture
            .layer_digests
            .first()
            .ok_or("large layer digest is missing")?,
        _ => return Err("unsupported blob kind".into()),
    };
    let expected_path = blob_path(digest);
    fixture.set_remote_blob_fault(kind, fault)?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let expected = fixture
        .expected_inventory
        .get(&expected_path)
        .ok_or("readback fixture blob is missing")?;
    let foreign = fixture.scratch.join("foreign.keep");
    fs::write(&foreign, b"preserve")?;
    let output = fixture.invoke()?;
    assert!(
        !output.status.success(),
        "accepted {kind} blob fault {fault:?}"
    );
    assert!(fixture.last_fixture_error()?.is_none());
    let error = match fault {
        Fault::Missing | Fault::Nonzero => "image_publication_blob_process",
        Fault::Short => "image_publication_blob_size",
        Fault::Oversize => "image_publication_crane_process",
        Fault::CorruptAt(_) => "image_publication_blob_mismatch",
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(error),
        "unexpected {kind} readback rejection for {fault:?}: {stderr}"
    );
    assert!(!fixture.receipt.exists());
    assert_eq!(
        fixture.registry_digest()?,
        Some(fixture.expected_digest.clone())
    );
    let actual_inventory = fixture.stored_inventory()?;
    let state = fixture.registry_state()?;
    let remote_entry = state["remote"]["inventory"].get(&expected_path);
    match fault {
        Fault::Missing => {
            assert!(!actual_inventory.contains_key(&expected_path));
            assert!(remote_entry.is_none());
        }
        Fault::Short | Fault::Oversize | Fault::CorruptAt(_) | Fault::Nonzero => {
            let actual = actual_inventory
                .get(&expected_path)
                .ok_or("faulted remote blob is missing")?;
            let entry = remote_entry.ok_or("faulted remote inventory entry is missing")?;
            assert_eq!(entry["size"].as_u64(), Some(u64::try_from(actual.len())?));
            match fault {
                Fault::Short => assert_eq!(actual.len() + 1, expected.len()),
                Fault::Oversize => assert_eq!(actual.len(), expected.len() + 1),
                Fault::CorruptAt(_) => {
                    assert_eq!(actual.len(), expected.len());
                    assert_ne!(actual, expected);
                }
                Fault::Nonzero => assert_eq!(actual, expected),
                Fault::Missing => return Err("missing fault has an inventory entry".into()),
            }
        }
    }
    assert_eq!(fs::read(&foreign)?, b"preserve");
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
    assert_eq!(fs::read(&fixture.archive)?, archive_before);
    assert_eq!(fs::read(&fixture.record)?, record_before);
    Ok(())
}

fn blob_path(digest: &str) -> String {
    format!("blobs/sha256/{}", digest.trim_start_matches("sha256:"))
}

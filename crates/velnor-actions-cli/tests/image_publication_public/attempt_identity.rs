// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use serde_json::{Value, json};

use super::support::{
    AttemptArtifactCase as Case, Fixture, FixtureProfile, current_attestation,
    invalid_invocation_uri, with_invocation, with_uri,
};

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
fn public_runner_publication_selects_current_attestation_from_valid_history()
-> Result<(), Box<dyn Error>> {
    for ordering in 0..4 {
        let fixture = Fixture::new_for_attempt(FixtureProfile::Runner, 2)?;
        let current = current_attestation(&fixture)?;
        let historical_one = with_invocation(&current, fixture.producer_run_id, 1);
        let historical_two = with_invocation(&current, fixture.producer_run_id - 1, 3);
        let entries = match ordering {
            0 => vec![historical_one, current],
            1 => vec![current, historical_one],
            2 => vec![historical_one, historical_two, current],
            3 => vec![current, historical_one, historical_two],
            _ => return Err("invalid attestation ordering".into()),
        };
        fixture.set_attestation_output(json!(entries))?;

        let archive_before = fs::read(&fixture.archive)?;
        let record_before = fs::read(&fixture.record)?;
        let output = fixture.invoke()?;
        let fixture_error = fixture.last_fixture_error()?;
        assert!(
            output.status.success(),
            "ordering={ordering}: {}; fixture: {fixture_error:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(fixture_error.is_none());
        let receipt: Value = serde_json::from_slice(&fs::read(&fixture.receipt)?)?;
        assert_eq!(receipt["producer_run_attempt"], 2);
        assert_eq!(
            fixture.registry_digest()?,
            Some(fixture.expected_digest.clone())
        );
        assert_eq!(fixture.stored_inventory()?, fixture.expected_inventory);
        assert_eq!(fs::read(&fixture.archive)?, archive_before);
        assert_eq!(fs::read(&fixture.record)?, record_before);
        assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 0);
    }
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

#[test]
fn public_runner_publication_rejects_empty_and_malformed_attestation_output()
-> Result<(), Box<dyn Error>> {
    for (response, expected_error) in [
        (json!([]), "package_attestation_ambiguous"),
        (json!(null), "package_attestation_output_invalid"),
        (json!({}), "package_attestation_output_invalid"),
        (json!([null]), "package_attestation_output_invalid"),
    ] {
        assert_attestation_rejected(|_| Ok(response.clone()), expected_error)?;
    }
    Ok(())
}

#[test]
fn public_runner_publication_requires_exactly_one_current_attestation() -> Result<(), Box<dyn Error>>
{
    for duplicate in [false, true] {
        assert_attestation_rejected(
            |fixture| {
                let current = current_attestation(fixture)?;
                if duplicate {
                    Ok(json!([current.clone(), current]))
                } else {
                    Ok(json!([with_invocation(
                        &current,
                        fixture.producer_run_id,
                        1
                    )]))
                }
            },
            if duplicate {
                "package_attestation_ambiguous"
            } else {
                "package_attestation_run_identity_mismatch"
            },
        )?;
    }
    Ok(())
}

#[test]
fn public_runner_publication_rejects_invalid_attestation_entries_in_both_orders()
-> Result<(), Box<dyn Error>> {
    for (invalid, expected_error) in [
        ("predicate", "package_attestation_predicate_mismatch"),
        ("event", "package_attestation_event_mismatch"),
        ("source", "package_attestation_identity_mismatch"),
        (
            "inventory",
            "package_attestation_subject_inventory_mismatch",
        ),
        ("digest", "package_attestation_subject_inventory_invalid"),
        ("malformed", "package_attestation_output_invalid"),
    ] {
        for invalid_first in [true, false] {
            assert_attestation_rejected(
                |fixture| {
                    let current = current_attestation(fixture)?;
                    let mut invalid_entry = with_invocation(&current, fixture.producer_run_id, 1);
                    match invalid {
                        "predicate" => {
                            invalid_entry["verificationResult"]["statement"]["predicateType"] =
                                json!("https://invalid.example/predicate");
                        }
                        "event" => {
                            invalid_entry["verificationResult"]["statement"]["predicate"]["buildDefinition"]
                                ["internalParameters"]["github"]["event_name"] = json!("push");
                        }
                        "source" => {
                            invalid_entry["verificationResult"]["signature"]["certificate"]["sourceRepositoryDigest"] =
                                json!("c".repeat(40));
                        }
                        "inventory" => {
                            let subjects =
                                invalid_entry["verificationResult"]["statement"]["subject"]
                                    .as_array_mut()
                                    .ok_or("fixture attestation subjects are missing")?;
                            subjects
                                .retain(|subject| subject["name"] != fixture.profile.record_name());
                        }
                        "digest" => {
                            let subjects =
                                invalid_entry["verificationResult"]["statement"]["subject"]
                                    .as_array_mut()
                                    .ok_or("fixture attestation subjects are missing")?;
                            let record = subjects
                                .iter_mut()
                                .find(|subject| subject["name"] == fixture.profile.record_name())
                                .ok_or("fixture record subject is missing")?;
                            record["digest"]["sha256"] = json!("z".repeat(64));
                        }
                        "malformed" => {
                            invalid_entry["verificationResult"]["signature"]["certificate"]
                                .as_object_mut()
                                .ok_or("fixture certificate is missing")?
                                .remove("runnerEnvironment");
                        }
                        _ => return Err("unknown invalid attestation case".into()),
                    }
                    let entries = if invalid_first {
                        vec![invalid_entry, current]
                    } else {
                        vec![current, invalid_entry]
                    };
                    Ok(json!(entries))
                },
                expected_error,
            )?;
        }
    }
    Ok(())
}

#[test]
fn public_runner_publication_rejects_each_non_run_certificate_field_in_both_orders()
-> Result<(), Box<dyn Error>> {
    for field in [
        "issuer",
        "subjectAlternativeName",
        "sourceRepositoryURI",
        "sourceRepositoryOwnerURI",
        "sourceRepositoryIdentifier",
        "sourceRepositoryDigest",
        "sourceRepositoryRef",
        "buildSignerURI",
        "buildSignerDigest",
        "buildConfigURI",
        "buildConfigDigest",
        "buildTrigger",
        "runnerEnvironment",
    ] {
        for invalid_first in [true, false] {
            assert_attestation_rejected(
                |fixture| {
                    let current = current_attestation(fixture)?;
                    let mut invalid_entry = with_invocation(&current, fixture.producer_run_id, 1);
                    invalid_entry["verificationResult"]["signature"]["certificate"][field] =
                        json!(format!("invalid-{field}"));
                    let entries = if invalid_first {
                        vec![invalid_entry, current]
                    } else {
                        vec![current, invalid_entry]
                    };
                    Ok(json!(entries))
                },
                "package_attestation_identity_mismatch",
            )?;
        }
    }
    Ok(())
}

#[test]
fn public_runner_publication_rejects_noncanonical_attestation_run_uris_in_both_orders()
-> Result<(), Box<dyn Error>> {
    for uri_case in [
        "foreign",
        "query",
        "fragment",
        "trailing_slash",
        "zero_run",
        "zero_attempt",
        "leading_zero_run",
        "leading_zero_attempt",
        "overflow_run",
        "overflow_attempt",
    ] {
        for invalid_first in [true, false] {
            assert_attestation_rejected(
                |fixture| {
                    let current = current_attestation(fixture)?;
                    let invalid_entry =
                        with_uri(&current, &invalid_invocation_uri(fixture, uri_case));
                    let entries = if invalid_first {
                        vec![invalid_entry, current]
                    } else {
                        vec![current, invalid_entry]
                    };
                    Ok(json!(entries))
                },
                "package_attestation_run_identity_mismatch",
            )?;
        }
    }
    Ok(())
}

fn assert_attestation_rejected(
    response: impl FnOnce(&Fixture) -> Result<Value, Box<dyn Error>>,
    expected_error: &str,
) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new_for_attempt(FixtureProfile::Runner, 2)?;
    fixture.set_attestation_output(response(&fixture)?)?;
    let foreign = fixture.scratch.join("foreign.keep");
    fs::write(&foreign, b"preserve")?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let registry_before = fixture.preseed_registry_state()?;

    let output = fixture.invoke()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "accepted {expected_error}");
    assert!(
        stderr.contains(expected_error),
        "wrong production rejection: expected {expected_error}; got {stderr}"
    );
    assert!(fixture.last_fixture_error()?.is_none());
    assert!(!fixture.receipt.exists());
    assert_eq!(fixture.registry_state()?, registry_before);
    assert_eq!(fs::read(&fixture.archive)?, archive_before);
    assert_eq!(fs::read(&fixture.record)?, record_before);
    assert_eq!(fs::read(&foreign)?, b"preserve");
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
    Ok(())
}

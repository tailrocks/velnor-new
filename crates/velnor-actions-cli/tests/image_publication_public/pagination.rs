// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use serde_json::{Value, json};

use super::support::{Fixture, FixtureProfile, current_attestation, with_invocation};

struct PaginatedRejectionCase {
    pages: Value,
    expected_error: &'static str,
    expected_fixture_error: Option<&'static str>,
}

#[test]
fn public_runner_publication_selects_expected_attestation_after_pagination()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new_for_attempt(FixtureProfile::Job, 2)?;
    let current = current_attestation(&fixture)?;
    let run_id = fixture.profile.run_id();
    let mut previous = (1..=100)
        .map(|offset| {
            Ok(with_invocation(
                &current,
                run_id
                    .checked_sub(offset)
                    .ok_or("fixture run ID cannot identify stale attempts")?,
                1,
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    previous[0]["verificationResult"]["statement"]["predicate"]["buildDefinition"]
        ["internalParameters"]
        .as_object_mut()
        .ok_or("stale fixture internal parameters are missing")?
        .remove("github");
    fixture.set_attestation_pages(json!([
        {"attestations": previous.iter().map(bundle).collect::<Vec<_>>()},
        {"attestations": [bundle(&current)]},
    ]))?;

    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let output = fixture.invoke()?;
    let fixture_error = fixture.last_fixture_error()?;
    assert!(
        output.status.success(),
        "paginated attestation selection failed: {}; fixture: {fixture_error:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture_error.is_none());
    let receipt: Value = super::support::read_receipt(&fixture.receipt)?;
    assert_eq!(receipt["producer_run_attempt"], 2);
    assert_eq!(
        fixture.registry_digest()?,
        Some(fixture.expected_digest.clone())
    );
    assert_eq!(fixture.stored_inventory()?, fixture.expected_inventory);
    assert_eq!(fs::read(&fixture.archive)?, archive_before);
    assert_eq!(fs::read(&fixture.record)?, record_before);
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 0);
    Ok(())
}

#[test]
fn public_runner_publication_rejects_unusable_paginated_attestations_before_registry_write()
-> Result<(), Box<dyn Error>> {
    for case in paginated_rejection_cases()? {
        let fixture = Fixture::new_for_attempt(FixtureProfile::Job, 2)?;
        fixture.set_attestation_pages(case.pages)?;
        let registry_before = fixture.preseed_registry_state()?;
        let archive_before = fs::read(&fixture.archive)?;
        let record_before = fs::read(&fixture.record)?;

        let output = fixture.invoke()?;
        let fixture_error = fixture.last_fixture_error()?;
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(case.expected_error),
            "wrong attestation rejection: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fixture_error.as_deref(), case.expected_fixture_error);
        assert!(!fixture.receipt.exists());
        assert_eq!(fixture.registry_state()?, registry_before);
        assert_eq!(fs::read(&fixture.archive)?, archive_before);
        assert_eq!(fs::read(&fixture.record)?, record_before);
        assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 0);
    }
    Ok(())
}

fn paginated_rejection_cases() -> Result<Vec<PaginatedRejectionCase>, Box<dyn Error>> {
    let fixture = Fixture::new_for_attempt(FixtureProfile::Job, 2)?;
    let current = current_attestation(&fixture)?;
    let run_id = fixture.profile.run_id();
    let stale = (1..=100)
        .map(|offset| {
            let old_run = run_id
                .checked_sub(offset)
                .ok_or("fixture run ID cannot identify stale attempts")?;
            Ok(with_invocation(&current, old_run, 1))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let wrong_attempt = with_invocation(&current, run_id, 1);
    let mut malformed_current = current.clone();
    malformed_current["verificationResult"]["statement"]["predicate"]["buildDefinition"]
        ["internalParameters"]
        .as_object_mut()
        .ok_or("current fixture internal parameters are missing")?
        .remove("github");
    Ok(vec![
        PaginatedRejectionCase {
            pages: json!([
                {"attestations": stale.iter().map(bundle).collect::<Vec<_>>()},
                {"attestations": [bundle(&wrong_attempt)]},
            ]),
            expected_error: "package_attestation_run_identity_mismatch",
            expected_fixture_error: None,
        },
        PaginatedRejectionCase {
            pages: json!([{"attestations": [{"bundle": null}]}]),
            expected_error: "package_attestation_bundle_invalid",
            expected_fixture_error: None,
        },
        PaginatedRejectionCase {
            pages: json!([{"attestations": [{"bundle": null, "bundle_url": "http://127.0.0.1/bundle.json.sn"}]}]),
            expected_error: "github_evidence_archive_location",
            expected_fixture_error: None,
        },
        PaginatedRejectionCase {
            pages: json!([{"attestations": [bundle(&malformed_current)]}]),
            expected_error: "package_attestation_output_invalid",
            expected_fixture_error: None,
        },
    ])
}

fn bundle(output: &Value) -> Value {
    json!({"bundle":{"fixtureOutput":[output]}})
}

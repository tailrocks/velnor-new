// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use serde_json::{Value, json};

use super::support::{Fixture, FixtureProfile};

struct PaginatedRejectionCase {
    pages: Value,
    expected_error: &'static str,
    expected_fixture_error: Option<&'static str>,
}

#[test]
fn public_runner_publication_rejects_unusable_paginated_attestations_before_registry_write()
-> Result<(), Box<dyn Error>> {
    for case in paginated_rejection_cases() {
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

fn paginated_rejection_cases() -> Vec<PaginatedRejectionCase> {
    vec![
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
    ]
}

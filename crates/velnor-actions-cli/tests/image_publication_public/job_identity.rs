// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use serde_json::json;

use super::support::Fixture;

#[test]
fn boundary_job_publication_rejects_wrong_image_identity_before_registry_writes()
-> Result<(), Box<dyn Error>> {
    super::run_isolated(
        "job_identity::boundary_job_publication_rejects_wrong_image_identity_before_registry_writes",
        || {
            for (revision, kind, expected_error) in [
                ("a".repeat(40), "runner", "image_publication_image_kind"),
                ("b".repeat(40), "job", "runner_image_revision"),
            ] {
                let fixture = Fixture::new_with_image_config(Some(json!({
                    "Labels": {
                        "org.opencontainers.image.revision": revision,
                        "io.velnor.image.kind": kind,
                    },
                    "User": "fixture",
                })))?;
                let before = fixture.registry_state()?;
                let output = fixture.invoke_publication_boundary()?;
                assert!(!output.status.success());
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains(expected_error),
                    "unexpected Job image identity error: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(fixture.registry_state()?, before);
                assert!(fixture.last_fixture_error()?.is_none());
            }
            Ok(())
        },
    )
}

#[test]
fn boundary_job_publication_rejects_unlabeled_or_malformed_configuration()
-> Result<(), Box<dyn Error>> {
    super::run_isolated(
        "job_identity::boundary_job_publication_rejects_unlabeled_or_malformed_configuration",
        || {
            for (config, expected) in [
                (json!({"User":"fixture"}), "runner_image_revision"),
                (json!({"Cmd":17}), "image_publication_oci_invalid"),
            ] {
                let fixture = Fixture::new_with_image_config(Some(config))?;
                let archive_before = fs::read(&fixture.archive)?;
                let record_before = fs::read(&fixture.record)?;
                let registry_before = fixture.registry_state()?;
                let output = fixture.invoke_publication_boundary()?;
                let fixture_error = fixture.last_fixture_error()?;
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(!output.status.success());
                assert!(
                    stderr.contains(expected),
                    "expected {expected}; got {stderr}"
                );
                assert!(!fixture.receipt.exists());
                assert!(fixture_error.is_none());
                assert_eq!(fixture.registry_state()?, registry_before);
                assert_eq!(fs::read(&fixture.archive)?, archive_before);
                assert_eq!(fs::read(&fixture.record)?, record_before);
                assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 0);
            }
            Ok(())
        },
    )
}

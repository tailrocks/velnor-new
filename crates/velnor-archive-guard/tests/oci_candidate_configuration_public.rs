// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    inspect_oci_candidate, inspect_oci_candidate_with_config,
};

#[path = "oci_candidate_configuration_public/support.rs"]
mod support;

#[test]
fn configuration_projection_preserves_authenticated_evidence_and_values()
-> Result<(), Box<dyn Error>> {
    let fixture = support::fixture(
        r#"{"Labels":{"org.opencontainers.image.revision":"ffffffffffffffffffffffffffffffffffffffff","release":"candidate"},"User":"1001:1001","WorkingDir":"/runner","Entrypoint":["/usr/bin/Runner.Listener"],"Cmd":["--once"],"Env":["LANG=C.UTF-8"]}"#,
        false,
    )?;
    let projected = inspect_oci_candidate_with_config(&fixture.path)?;
    let legacy = inspect_oci_candidate(&fixture.path)?;

    assert_eq!(projected.evidence, legacy);
    assert_eq!(projected.evidence.archive_sha256.len(), 64);
    assert_eq!(projected.evidence.platform_manifest_digest.len(), 71);
    assert_eq!(projected.evidence.image_configuration_digest.len(), 71);
    assert_eq!(projected.evidence.layer_digests.len(), 1);
    assert_eq!(
        projected
            .configuration
            .labels
            .as_ref()
            .and_then(|labels| labels.get("release"))
            .map(String::as_str),
        Some("candidate")
    );
    assert_eq!(
        projected
            .configuration
            .labels
            .as_ref()
            .and_then(|labels| labels.get("org.opencontainers.image.revision"))
            .map(String::as_str),
        Some("ffffffffffffffffffffffffffffffffffffffff")
    );
    assert_eq!(projected.configuration.user.as_deref(), Some("1001:1001"));
    assert_eq!(
        projected.configuration.working_directory.as_deref(),
        Some("/runner")
    );
    assert_eq!(
        projected.configuration.entrypoint,
        Some(vec!["/usr/bin/Runner.Listener".to_owned()])
    );
    assert_eq!(projected.configuration.cmd, Some(vec!["--once".to_owned()]));
    assert_eq!(
        projected.configuration.env,
        Some(vec!["LANG=C.UTF-8".to_owned()])
    );
    Ok(())
}

#[test]
fn absent_null_and_empty_configuration_values_remain_distinct() -> Result<(), Box<dyn Error>> {
    let absent = support::fixture("{}", false)?;
    let null_configuration = support::fixture("null", false)?;
    let null = support::fixture(
        r#"{"Labels":null,"User":null,"WorkingDir":null,"Entrypoint":null,"Cmd":null,"Env":null}"#,
        false,
    )?;
    let empty = support::fixture(
        r#"{"Labels":{},"User":"","WorkingDir":"","Entrypoint":[],"Cmd":[],"Env":[]}"#,
        false,
    )?;
    let absent = inspect_oci_candidate_with_config(&absent.path)?.configuration;
    let null = inspect_oci_candidate_with_config(&null.path)?.configuration;
    let empty = inspect_oci_candidate_with_config(&empty.path)?.configuration;

    assert_eq!(absent, null);
    assert_eq!(
        absent,
        inspect_oci_candidate_with_config(&null_configuration.path)?.configuration
    );
    assert_eq!(absent.labels, None);
    assert_eq!(absent.user, None);
    assert_eq!(absent.working_directory, None);
    assert_eq!(absent.entrypoint, None);
    assert_eq!(absent.cmd, None);
    assert_eq!(absent.env, None);
    assert_eq!(empty.labels, Some(std::collections::BTreeMap::new()));
    assert_eq!(empty.user.as_deref(), Some(""));
    assert_eq!(empty.working_directory.as_deref(), Some(""));
    assert_eq!(empty.entrypoint, Some(Vec::new()));
    assert_eq!(empty.cmd, Some(Vec::new()));
    assert_eq!(empty.env, Some(Vec::new()));
    Ok(())
}

#[test]
fn legacy_inspection_ignores_malformed_optional_projection_fields() -> Result<(), Box<dyn Error>> {
    let malformed = [
        "false",
        r#"{"Labels":[]}"#,
        r#"{"Labels":{"key":1}}"#,
        r#"{"User":[]}"#,
        r#"{"WorkingDir":false}"#,
        r#"{"Entrypoint":"not-an-array"}"#,
        r#"{"Cmd":[1]}"#,
        r#"{"Env":{}}"#,
    ];
    for projection in malformed {
        let fixture = support::fixture(projection, false)?;
        assert!(
            inspect_oci_candidate(&fixture.path).is_ok(),
            "legacy inspection rejected projection {projection}"
        );
        assert!(
            inspect_oci_candidate_with_config(&fixture.path).is_err(),
            "richer inspection accepted projection {projection}"
        );
    }
    Ok(())
}

#[test]
fn strict_json_duplicates_and_tampered_config_are_rejected_by_both_apis()
-> Result<(), Box<dyn Error>> {
    let duplicate = support::fixture(r#"{"User":"first","User":"second"}"#, false)?;
    let tampered = support::fixture(r#"{"User":"runner"}"#, true)?;

    assert!(inspect_oci_candidate(&duplicate.path).is_err());
    assert!(inspect_oci_candidate_with_config(&duplicate.path).is_err());
    assert!(inspect_oci_candidate(&tampered.path).is_err());
    assert!(inspect_oci_candidate_with_config(&tampered.path).is_err());
    Ok(())
}

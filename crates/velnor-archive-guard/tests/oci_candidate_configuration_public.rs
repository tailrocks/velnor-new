// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error, fs, path::Path};

use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    inspect_oci_candidate, inspect_oci_candidate_with_config, materialize_oci_candidate,
};

#[path = "oci_candidate_configuration_public/support.rs"]
mod support;

#[path = "oci_candidate_configuration_public/deadline.rs"]
mod deadline;

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
        projected.configuration.config_keys,
        ["Cmd", "Entrypoint", "Env", "Labels", "User", "WorkingDir"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
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
    let absent_fixture = support::fixture("{}", false)?;
    let null_config_fixture = support::fixture("null", false)?;
    let null_fields_fixture = support::fixture(
        r#"{"Labels":null,"User":null,"WorkingDir":null,"Entrypoint":null,"Cmd":null,"Env":null}"#,
        false,
    )?;
    let empty_fixture = support::fixture(
        r#"{"Labels":{},"User":"","WorkingDir":"","Entrypoint":[],"Cmd":[],"Env":[]}"#,
        false,
    )?;
    let absent = inspect_oci_candidate_with_config(&absent_fixture.path)?.configuration;
    let null = inspect_oci_candidate_with_config(&null_config_fixture.path)?.configuration;
    let null_fields = inspect_oci_candidate_with_config(&null_fields_fixture.path)?.configuration;
    let empty = inspect_oci_candidate_with_config(&empty_fixture.path)?.configuration;

    assert_eq!(absent, null);
    assert_eq!(absent.config_keys, std::collections::BTreeSet::new());
    assert_eq!(null.config_keys, std::collections::BTreeSet::new());
    assert_eq!(null_fields.config_keys.len(), 6);
    assert_eq!(null_fields.labels, absent.labels);
    assert_eq!(null_fields.user, absent.user);
    assert_eq!(null_fields.working_directory, absent.working_directory);
    assert_eq!(null_fields.entrypoint, absent.entrypoint);
    assert_eq!(null_fields.cmd, absent.cmd);
    assert_eq!(null_fields.env, absent.env);
    assert_eq!(
        inspect_oci_candidate_with_config(&null_config_fixture.path)?
            .configuration
            .config_keys,
        std::collections::BTreeSet::new()
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
    assert_eq!(empty.config_keys, null_fields.config_keys);
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
        let inspected = inspect_oci_candidate(&fixture.path);
        assert!(
            inspected.is_ok(),
            "legacy inspection rejected projection {projection}"
        );
        let evidence = inspected?;
        assert!(
            inspect_oci_candidate_with_config(&fixture.path).is_err(),
            "richer inspection accepted projection {projection}"
        );
        let scratch = tempfile::tempdir()?;
        let destination = scratch.path().join("candidate");
        assert_eq!(
            materialize_oci_candidate(&fixture.path, &destination, &evidence)?,
            evidence,
            "materializer rejected projection {projection} accepted by legacy inspection"
        );
        assert_eq!(
            projection_files(&destination)?,
            support::archive_files(&fixture.path)?,
            "materializer changed bytes for projection {projection}"
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

#[test]
fn materialization_preserves_noncanonical_metadata_and_all_blob_bytes() -> Result<(), Box<dyn Error>>
{
    let fixture = support::fixture_with_noncanonical_metadata(r#"{"User":"runner"}"#, false)?;
    let expected = inspect_oci_candidate(&fixture.path)?;
    let source = support::archive_files(&fixture.path)?;
    let scratch = tempfile::tempdir()?;
    let destination = scratch.path().join("candidate");

    let actual = materialize_oci_candidate(&fixture.path, &destination, &expected)?;

    assert_eq!(actual, expected);
    assert_eq!(projection_files(&destination)?, source);
    assert_eq!(
        directory_names(&destination)?,
        ["blobs", "index.json", "oci-layout"]
    );
    assert_eq!(directory_names(&destination.join("blobs"))?, ["sha256"]);
    assert_eq!(
        fs::read(destination.join("oci-layout"))?,
        b"{\n  \"imageLayoutVersion\" : \"1.0.0\"\n}\n"
    );
    Ok(())
}

#[test]
fn materialization_rejects_evidence_mismatch_and_valid_archive_substitution()
-> Result<(), Box<dyn Error>> {
    let first = support::fixture(r#"{"User":"first"}"#, false)?;
    let second = support::fixture(r#"{"User":"second"}"#, false)?;
    let expected = inspect_oci_candidate(&first.path)?;
    let scratch = tempfile::tempdir()?;
    let mismatch_destination = scratch.path().join("mismatch");
    let substitution_destination = scratch.path().join("substitution");
    let mut mismatched = expected.clone();
    mismatched.archive_size_bytes = mismatched
        .archive_size_bytes
        .checked_add(1)
        .ok_or("fixture archive size overflow")?;

    assert!(materialize_oci_candidate(&first.path, &mismatch_destination, &mismatched).is_err());
    assert!(!mismatch_destination.exists());
    assert!(materialize_oci_candidate(&second.path, &substitution_destination, &expected).is_err());
    assert!(!substitution_destination.exists());
    Ok(())
}

#[test]
fn materialization_rejects_malformed_archive_before_destination_creation()
-> Result<(), Box<dyn Error>> {
    let valid = support::fixture(r#"{"User":"runner"}"#, false)?;
    let tampered = support::fixture(r#"{"User":"runner"}"#, true)?;
    let expected = inspect_oci_candidate(&valid.path)?;
    let scratch = tempfile::tempdir()?;
    let destination = scratch.path().join("candidate");

    assert!(materialize_oci_candidate(&tampered.path, &destination, &expected).is_err());
    assert!(!destination.exists());
    Ok(())
}

#[test]
fn materialization_rejects_unsupported_layout_version_with_matching_archive_hash()
-> Result<(), Box<dyn Error>> {
    let fixture = support::fixture(r#"{"User":"runner"}"#, false)?;
    let original = inspect_oci_candidate(&fixture.path)?;
    let changed_archive_sha256 = support::change_layout_version(&fixture.path)?;
    let mut expected = original.clone();
    expected.archive_sha256 = changed_archive_sha256;
    let mut normalized = expected.clone();
    normalized.archive_sha256 = original.archive_sha256.clone();
    let scratch = tempfile::tempdir()?;
    let destination = scratch.path().join("candidate");

    assert_ne!(expected.archive_sha256, original.archive_sha256);
    assert_eq!(normalized, original);
    assert!(materialize_oci_candidate(&fixture.path, &destination, &expected).is_err());
    assert!(!destination.exists());
    Ok(())
}

#[test]
fn materialization_leaves_existing_file_and_directory_unchanged() -> Result<(), Box<dyn Error>> {
    let fixture = support::fixture(r#"{"User":"runner"}"#, false)?;
    let expected = inspect_oci_candidate(&fixture.path)?;
    let scratch = tempfile::tempdir()?;
    let file = scratch.path().join("existing-file");
    let directory = scratch.path().join("existing-directory");
    fs::write(&file, b"preserve-file")?;
    fs::create_dir(&directory)?;
    fs::write(directory.join("marker"), b"preserve-directory")?;

    assert!(materialize_oci_candidate(&fixture.path, &file, &expected).is_err());
    assert!(materialize_oci_candidate(&fixture.path, &directory, &expected).is_err());
    assert_eq!(fs::read(&file)?, b"preserve-file");
    assert_eq!(directory_names(&directory)?, ["marker"]);
    assert_eq!(fs::read(directory.join("marker"))?, b"preserve-directory");
    Ok(())
}

fn projection_files(directory: &Path) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
    let mut files = BTreeMap::new();
    for name in ["oci-layout", "index.json"] {
        files.insert(name.to_owned(), fs::read(directory.join(name))?);
    }
    let blobs = directory.join("blobs/sha256");
    for entry in fs::read_dir(blobs)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "projected blob name is not UTF-8")?;
        files.insert(format!("blobs/sha256/{name}"), fs::read(entry.path())?);
    }
    Ok(files)
}

fn directory_names(directory: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut names = fs::read_dir(directory)?
        .map(|entry| {
            entry?
                .file_name()
                .into_string()
                .map_err(|_| "directory entry name is not UTF-8".into())
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    names.sort();
    Ok(names)
}

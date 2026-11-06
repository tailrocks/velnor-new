// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    os::unix::ffi::OsStringExt,
    time::Instant,
};

use serde_json::{Value, json};

use super::{
    ASSET_LIMIT, Admission, Catalog, Context, FileIdentity, JSON_LIMIT, encode,
    handoff_fs::{read_artifact, read_regular},
    identity::{credential_name, valid_cf_encoding},
    require_unchanged_admission, sha256, validate_admission, validate_result,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[path = "tests/public_entry.rs"]
mod public_entry;
#[path = "tests/zip_bundle.rs"]
mod zip_bundle;

fn fixture() -> (Admission, Context, Catalog) {
    let context = Context {
        repository: "tailrocks/velnor-new".to_owned(),
        repository_id: 105,
        source_sha: "a".repeat(40),
        run_id: 204,
        run_attempt: 2,
        workflow_ref:
            "tailrocks/velnor-new/.github/workflows/generator-release.yml@refs/heads/main"
                .to_owned(),
        workflow_sha: "b".repeat(40),
        manifest_sha256: "c".repeat(64),
    };
    let catalog = Catalog {
        plan: "1".repeat(64),
        formula: "2".repeat(64),
        a_record: "3".repeat(64),
        d_record: "4".repeat(64),
        recovery: "5".repeat(64),
    };
    let admission = Admission {
        schema: "velnor.homebrew-qualification-admission.v1".to_owned(),
        context: context.clone(),
        catalog: catalog.clone(),
        authority_sha: "d".repeat(40),
        files: BTreeMap::from([(
            "A/release.json".to_owned(),
            FileIdentity {
                size: 7,
                sha256: sha256(b"receipt"),
            },
        )]),
    };
    (admission, context, catalog)
}

#[test]
fn admission_requires_exact_run_workflow_candidate_and_catalog() -> TestResult {
    let (admission, context, catalog) = fixture();
    let bytes = encode(&admission)?;
    assert_eq!(
        validate_admission(&bytes, &sha256(&bytes), &context, &catalog)?,
        admission
    );
    assert!(validate_admission(&bytes, &"0".repeat(64), &context, &catalog).is_err());
    let original = serde_json::to_value(&admission)?;
    for field in [
        "repository",
        "repository_id",
        "source_sha",
        "run_id",
        "run_attempt",
        "workflow_ref",
        "workflow_sha",
        "manifest_sha256",
    ] {
        let mut changed = original.clone();
        changed["context"][field] = if changed["context"][field].is_number() {
            json!(999)
        } else {
            json!("substituted")
        };
        let bytes = velnor_actions_contract::canonical_json_bytes(&changed)?;
        assert!(
            validate_admission(&bytes, &sha256(&bytes), &context, &catalog).is_err(),
            "accepted changed context {field}"
        );
    }
    for field in ["plan", "formula", "a_record", "d_record", "recovery"] {
        let mut changed = original.clone();
        changed["catalog"][field] = json!("f".repeat(64));
        let bytes = velnor_actions_contract::canonical_json_bytes(&changed)?;
        assert!(
            validate_admission(&bytes, &sha256(&bytes), &context, &catalog).is_err(),
            "accepted changed catalog {field}"
        );
    }
    Ok(())
}

#[test]
fn admission_refuses_ambiguous_unbounded_and_traversing_inputs() -> TestResult {
    let (admission, context, catalog) = fixture();
    let original = serde_json::to_value(&admission)?;
    let mut cases = vec![
        serde_json::to_vec_pretty(&original)?,
        vec![b' '; usize::try_from(JSON_LIMIT)? + 1],
    ];
    let bytes = encode(&admission)?;
    let duplicate = format!(
        "{{\"schema\":\"forged\",{}",
        std::str::from_utf8(&bytes)?
            .strip_prefix('{')
            .ok_or("fixture shape")?
    );
    cases.push(duplicate.into_bytes());
    for name in [
        "A/../outside",
        "../outside",
        "A//archive",
        "A/\\outside",
        "A/.",
        "D/..",
    ] {
        let mut changed = original.clone();
        changed["files"] = json!({name: {"size": 1, "sha256": "a".repeat(64)}});
        cases.push(velnor_actions_contract::canonical_json_bytes(&changed)?);
    }
    for (field, value) in [
        ("size", json!(0)),
        ("size", json!(ASSET_LIMIT + 1)),
        ("sha256", json!("bad")),
    ] {
        let mut changed = original.clone();
        changed["files"]["A/release.json"][field] = value;
        cases.push(velnor_actions_contract::canonical_json_bytes(&changed)?);
    }
    let mut unknown = original;
    unknown["unknown"] = Value::Bool(true);
    cases.push(velnor_actions_contract::canonical_json_bytes(&unknown)?);
    for bytes in cases {
        assert!(validate_admission(&bytes, &sha256(&bytes), &context, &catalog).is_err());
    }
    Ok(())
}

#[test]
fn admitted_files_refuse_substitution_extra_files_symlinks_and_size_changes() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("downloaded-artifact");
    fs::write(&path, b"receipt")?;
    let digest = sha256(b"receipt");
    assert_eq!(
        read_artifact(root.path(), &digest, 7, JSON_LIMIT)?,
        b"receipt"
    );
    fs::write(&path, b"forgery")?;
    assert!(read_artifact(root.path(), &digest, 7, JSON_LIMIT).is_err());
    fs::write(&path, b"oversize")?;
    assert!(read_artifact(root.path(), &digest, 7, JSON_LIMIT).is_err());
    fs::write(root.path().join("unexpected"), b"extra")?;
    assert!(read_artifact(root.path(), &digest, 7, JSON_LIMIT).is_err());
    fs::remove_file(root.path().join("unexpected"))?;
    fs::remove_file(&path)?;
    std::os::unix::fs::symlink("/etc/passwd", &path)?;
    assert!(read_regular(&path, JSON_LIMIT).is_err());
    assert!(read_artifact(root.path(), &digest, 7, JSON_LIMIT).is_err());
    assert!(read_artifact(root.path(), &digest, JSON_LIMIT + 1, JSON_LIMIT).is_err());
    Ok(())
}

fn archive(names: &[&str]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut archive = tar::Builder::new(gzip);
    for name in names {
        let mut header = tar::Header::new_gnu();
        let name_bytes = name.as_bytes();
        header.as_mut_bytes()[..name_bytes.len()].copy_from_slice(name_bytes);
        header.set_size(7);
        header.set_mode(0o755);
        header.set_cksum();
        archive.append(&header, &b"payload"[..])?;
    }
    archive.finish()?;
    let mut gzip = archive.into_inner()?;
    gzip.flush()?;
    Ok(gzip.finish()?)
}

#[test]
fn local_payload_preflight_rejects_duplicate_and_traversing_archive_members() -> TestResult {
    let expected = BTreeSet::from(["jackin".to_owned()]);
    let valid = archive(&["jackin"])?;
    let contents = crate::release::package_release_artifacts::extract_tar_gz_members(
        &valid,
        &expected,
        Instant::now(),
    )?;
    assert_eq!(contents.get("jackin"), Some(&b"payload".to_vec()));
    for members in [
        vec!["jackin", "jackin"],
        vec!["../jackin"],
        vec!["jackin", "unexpected"],
        vec!["/jackin"],
    ] {
        assert!(
            crate::release::package_release_artifacts::extract_tar_gz_members(
                &archive(&members)?,
                &expected,
                Instant::now()
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn local_execution_forbids_github_oidc_and_actions_runtime_credentials() {
    for name in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_RUNTIME_URL",
        "CUSTOM_SECRET",
        "CUSTOM_PASSWORD",
        "AWS_SECRET_ACCESS_KEY",
        "GITHUB_APP_PRIVATE_KEY",
    ] {
        assert!(credential_name(name), "accepted {name}");
    }
    for name in [
        "PATH",
        "HOME",
        "GITHUB_SHA",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "RUNNER_TEMP",
        "VELNOR_HOME_ADMISSION_SHA256",
    ] {
        assert!(!credential_name(name), "rejected {name}");
    }
}

#[test]
fn mac_platform_encoding_requires_three_hex_fields_and_the_current_owner() {
    assert!(valid_cf_encoding("0x1F5:0x0:0x0".as_ref(), 501));
    for value in [
        "0x1F6:0x0:0x0",
        "0x1F5:0x0",
        "0x1F5:0x0:0x0:0x0",
        "0x1F5:credential:0x0",
        "0x1F5:0x100000000:0x0",
    ] {
        assert!(!valid_cf_encoding(value.as_ref(), 501));
    }
    assert!(!valid_cf_encoding(
        &std::ffi::OsString::from_vec(vec![0xff]),
        501
    ));
}

#[test]
fn qualification_result_cannot_change_run_admission_or_success_semantics() -> TestResult {
    let (_, context, _) = fixture();
    let valid = json!({"schema": "velnor.homebrew-qualification-result.v1", "context": context, "admission_sha256": "e".repeat(64), "transition": "A-install-D-upgrade-preserved-kegs", "stable_sentinel": "disabled"});
    let bytes = velnor_actions_contract::canonical_json_bytes(&valid)?;
    validate_result(&bytes, &sha256(&bytes), &context, &"e".repeat(64))?;
    assert!(validate_result(&bytes, &"f".repeat(64), &context, &"e".repeat(64)).is_err());
    for field in [
        "schema",
        "admission_sha256",
        "transition",
        "stable_sentinel",
    ] {
        let mut changed = valid.clone();
        changed[field] = json!("forged");
        let bytes = velnor_actions_contract::canonical_json_bytes(&changed)?;
        assert!(validate_result(&bytes, &sha256(&bytes), &context, &"e".repeat(64)).is_err());
    }
    let mut changed = valid.clone();
    changed["context"]["run_attempt"] = json!(9);
    let bytes = velnor_actions_contract::canonical_json_bytes(&changed)?;
    assert!(validate_result(&bytes, &sha256(&bytes), &context, &"e".repeat(64)).is_err());
    let mut unknown = valid;
    unknown["ignored"] = json!(true);
    assert!(
        validate_result(
            &velnor_actions_contract::canonical_json_bytes(&unknown)?,
            &sha256(&velnor_actions_contract::canonical_json_bytes(&unknown)?),
            &context,
            &"e".repeat(64)
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn finalization_refuses_authority_tip_receipt_payload_and_catalog_drift() -> TestResult {
    let (admitted, _, _) = fixture();
    require_unchanged_admission(&admitted, &admitted)?;
    let original = serde_json::to_value(&admitted)?;
    let mut variants = Vec::new();
    let mut changed = original.clone();
    changed["authority_sha"] = json!("0".repeat(40));
    variants.push(changed);
    let mut changed = original.clone();
    changed["files"]["A/release.json"]["sha256"] = json!("0".repeat(64));
    variants.push(changed);
    let mut changed = original.clone();
    changed["files"]["D/payload.tar.gz"] = json!({"size": 5, "sha256": "a".repeat(64)});
    variants.push(changed);
    let mut changed = original;
    changed["catalog"]["plan"] = json!("0".repeat(64));
    variants.push(changed);
    for changed in variants {
        let fresh: Admission = serde_json::from_value(changed)?;
        assert!(require_unchanged_admission(&admitted, &fresh).is_err());
    }
    Ok(())
}

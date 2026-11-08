// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error};

use serde_json::{Value, json};
use velnor_actions_contract::{
    parse_runner_image_publication_record, parse_runner_publication_source_inputs,
    runner_publication_source_inputs,
};

fn valid_record(image: &str) -> Result<Value, Box<dyn Error>> {
    let source = "a".repeat(40);
    let digest = "0".repeat(64);
    let inputs = runner_publication_source_inputs()?
        .iter()
        .copied()
        .map(|path| (path.to_owned(), digest.clone()))
        .collect::<BTreeMap<_, _>>();
    let names: &[&str] = if image == "runner" {
        &[
            "resource_probe",
            "storage_init",
            "job_start_hook",
            "job_started_hook",
        ]
    } else {
        &["resource_probe"]
    };
    let contexts = names.iter().map(|name| (*name, json!({"path": format!("target/velnor-runner-family/{name}"), "sha256": digest, "size_bytes": 1}))).collect::<BTreeMap<_, _>>();
    Ok(json!({
        "schema": "velnor.runner-image-publication.v2", "image": image,
        "source": {"commit": source, "ref": "refs/heads/main", "inputs": inputs},
        "workflow": {"repository": "tailrocks/velnor-new", "repository_id": 1_390_620_900,
            "event": "push", "workflow_ref": "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
            "authority_commit": source, "run_id": 40_613_392_798_u64, "run_attempt": 1},
        "platform": "linux/amd64", "archive": {"sha256": digest, "size_bytes": 1},
        "oci": {"archive_index_digest": digest, "archive_index_size_bytes": 1,
            "platform_manifest_digest": format!("sha256:{digest}"), "platform_manifest_size_bytes": 1,
            "image_configuration_digest": format!("sha256:{digest}"), "image_configuration_size_bytes": 1,
            "layer_digests": [format!("sha256:{digest}")]},
        "provenance": {"envelope": {"sha256": digest, "size_bytes": 1},
            "original_archive": {"sha256": digest, "size_bytes": 1},
            "original_index": {"sha256": digest, "size_bytes": 1},
            "named_contexts": contexts},
        "lock": {"sha256": digest, "runner_version": "2.338.0", "docker_version": "29.9.1",
            "buildx_version": "0.38.0", "compose_version": "5.6.0", "node_version": "26.12.0"}
    }))
}

#[test]
fn source_inventory_rejects_duplicate_and_control_paths() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        parse_runner_publication_source_inputs("a\nb\n")?,
        ["a", "b"]
    );
    for invalid in ["a\na\n", "a\0b\n", "a\rb\n", "a/../b\n"] {
        assert!(parse_runner_publication_source_inputs(invalid).is_err());
    }
    Ok(())
}

fn assert_rejected(record: &Value, case: &str) -> Result<(), Box<dyn Error>> {
    assert!(
        parse_runner_image_publication_record(&serde_json::to_vec(record)?).is_err(),
        "accepted {case}"
    );
    Ok(())
}

#[test]
fn accepts_each_profile_bound_to_default_branch_ci_source() -> Result<(), Box<dyn Error>> {
    for image in ["runner", "dind", "resource_probe"] {
        let record =
            parse_runner_image_publication_record(&serde_json::to_vec(&valid_record(image)?)?)?;
        assert_eq!(record.source.commit, record.workflow.authority_commit);
    }
    Ok(())
}

#[test]
fn rejects_untrusted_producer_and_legacy_qualification_claims() -> Result<(), Box<dyn Error>> {
    for (field, value) in [
        ("repository", json!("other/repo")),
        ("repository_id", json!(1)),
        ("event", json!("workflow_dispatch")),
        ("workflow_ref", json!("other")),
        ("authority_commit", json!("b".repeat(40))),
        ("run_id", json!(0)),
        ("run_attempt", json!(0)),
    ] {
        let mut record = valid_record("runner")?;
        record["workflow"][field] = value;
        assert_rejected(&record, field)?;
    }
    for field in ["qualification", "referenced_workflow"] {
        let mut record = valid_record("runner")?;
        record[field] = json!({});
        assert_rejected(&record, field)?;
    }
    let mut reference = valid_record("runner")?;
    reference["source"]["ref"] = json!("refs/heads/other");
    assert_rejected(&reference, "source ref")?;
    Ok(())
}

#[test]
fn rejects_invalid_profile_source_lock_and_image_identities() -> Result<(), Box<dyn Error>> {
    let mut profile = valid_record("runner")?;
    profile["image"] = json!("unknown");
    assert_rejected(&profile, "unknown profile")?;
    let mut lock = valid_record("runner")?;
    lock["lock"]["sha256"] = json!("e".repeat(64));
    assert_rejected(&lock, "lock source mismatch")?;
    let mut missing_lock = valid_record("runner")?;
    missing_lock
        .as_object_mut()
        .ok_or("record missing")?
        .remove("lock");
    assert_rejected(&missing_lock, "missing lock")?;
    let mut platform = valid_record("runner")?;
    platform["platform"] = json!("linux/arm64");
    assert_rejected(&platform, "wrong platform")?;
    let mut unknown = valid_record("runner")?;
    unknown["source"]["unexpected"] = json!(true);
    assert_rejected(&unknown, "unknown source field")?;
    let mut version = valid_record("runner")?;
    version["lock"]["buildx_version"] = json!("v0.38.0");
    assert_rejected(&version, "buildx version")?;
    let mut inputs = valid_record("runner")?;
    inputs["source"]["inputs"]
        .as_object_mut()
        .ok_or("inputs missing")?
        .remove(".velnor/runner.lock");
    assert_rejected(&inputs, "incomplete source inputs")?;
    let mut archive = valid_record("runner")?;
    archive["archive"]["size_bytes"] = json!(0);
    assert_rejected(&archive, "empty archive")?;
    let mut oci = valid_record("runner")?;
    oci["oci"]["image_configuration_digest"] = json!("invalid");
    assert_rejected(&oci, "invalid OCI digest")?;
    let valid = serde_json::to_string(&valid_record("runner")?)?;
    let duplicate = valid.replacen(
        "\"schema\":",
        "\"schema\":\"velnor.runner-image-publication.v2\",\"schema\":",
        1,
    );
    assert!(
        parse_runner_image_publication_record(duplicate.as_bytes()).is_err(),
        "accepted duplicate schema"
    );
    Ok(())
}

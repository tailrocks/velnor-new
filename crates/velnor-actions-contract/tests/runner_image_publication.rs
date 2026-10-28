// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error};

use serde_json::{Value, json};
use velnor_actions_contract::{
    RUNNER_PUBLICATION_SOURCE_INPUTS, parse_runner_image_publication_record,
};

fn valid_record(image: &str) -> Value {
    let source = "a".repeat(40);
    let caller = "b".repeat(40);
    let digest = "0".repeat(64);
    let inputs = RUNNER_PUBLICATION_SOURCE_INPUTS
        .into_iter()
        .map(|path| (path.to_owned(), digest.clone()))
        .collect::<BTreeMap<_, _>>();
    json!({
        "schema": "velnor.runner-image-publication.v1",
        "image": image,
        "source": {"commit": source, "ref": "refs/heads/main", "inputs": inputs},
        "workflow": {
            "repository": "tailrocks/velnor-new",
            "repository_id": 1_390_620_900,
            "event": "workflow_dispatch",
            "workflow_ref": "tailrocks/velnor-new/.github/workflows/product-release.yml@refs/heads/main",
            "authority_commit": caller,
            "run_id": 40_613_392_798_u64,
            "run_attempt": 1,
        },
        "referenced_workflow": {
            "path": format!("tailrocks/velnor-new/.github/workflows/product-release-images.yml@{caller}"),
            "ref": "refs/heads/main",
            "sha": caller,
        },
        "platform": "linux/amd64",
        "archive": {"sha256": digest, "size_bytes": 1},
        "oci": {
            "archive_index_digest": digest,
            "archive_index_size_bytes": 1,
            "platform_manifest_digest": format!("sha256:{digest}"),
            "platform_manifest_size_bytes": 1,
            "image_configuration_digest": format!("sha256:{digest}"),
            "image_configuration_size_bytes": 1,
            "layer_digests": [format!("sha256:{digest}")],
        },
        "lock": {
            "sha256": digest,
            "runner_version": "2.338.0",
            "docker_version": "29.9.1",
            "buildx_version": "0.38.0",
            "compose_version": "5.6.0",
            "node_version": "26.12.0",
        },
        "qualification": {
            "job_id": "qualify-images",
            "run_id": 40_613_392_798_u64,
            "run_attempt": 1,
            "conclusion": "success",
        }
    })
}

fn assert_rejected(record: &Value, case: &str) -> Result<(), Box<dyn Error>> {
    let bytes = serde_json::to_vec(record)?;
    assert!(
        parse_runner_image_publication_record(&bytes).is_err(),
        "accepted {case}"
    );
    Ok(())
}

#[test]
fn accepts_all_runner_profiles_with_a_distinct_selected_source_and_caller()
-> Result<(), Box<dyn Error>> {
    for image in ["runner", "dind", "resource_probe"] {
        let bytes = serde_json::to_vec(&valid_record(image))?;
        let record = parse_runner_image_publication_record(&bytes)?;
        assert_ne!(record.source.commit, record.workflow.authority_commit);
        assert_eq!(
            record.referenced_workflow.path,
            format!(
                "tailrocks/velnor-new/.github/workflows/product-release-images.yml@{}",
                record.workflow.authority_commit
            )
        );
        assert_eq!(record.referenced_workflow.reference, "refs/heads/main");
        assert_eq!(record.qualification.run_id, record.workflow.run_id);
    }
    Ok(())
}

#[test]
fn rejects_referenced_workflow_path_ref_coherent_sha_and_exact_version_mismatches()
-> Result<(), Box<dyn Error>> {
    let mut path = valid_record("runner");
    path["referenced_workflow"]["path"] = json!(".github/workflows/product-release-images.yml");
    let mut reference = valid_record("runner");
    reference["referenced_workflow"]["ref"] = json!("refs/heads/other");
    let mut sha = valid_record("runner");
    let changed_sha = "c".repeat(40);
    sha["referenced_workflow"]["sha"] = json!(changed_sha);
    sha["referenced_workflow"]["path"] = json!(format!(
        "tailrocks/velnor-new/.github/workflows/product-release-images.yml@{}",
        sha["referenced_workflow"]["sha"]
            .as_str()
            .expect("reference SHA")
    ));
    let mut version = valid_record("runner");
    version["lock"]["buildx_version"] = json!("v0.38.0");

    for (case, record) in [
        ("workflow path", path),
        ("workflow ref", reference),
        ("coherent workflow SHA", sha),
        ("exact buildx version", version),
    ] {
        assert_rejected(&record, case)?;
    }
    Ok(())
}

#[test]
fn rejects_unknown_profile_and_lock_source_digest_disagreement() -> Result<(), Box<dyn Error>> {
    let mut profile = valid_record("runner");
    profile["image"] = json!("unknown");
    let mut lock = valid_record("runner");
    lock["lock"]["sha256"] = json!("e".repeat(64));

    assert_rejected(&profile, "unknown image profile")?;
    assert_rejected(&lock, "lock digest that differs from the source input")?;
    Ok(())
}

#[test]
fn rejects_qualification_run_attempt_and_conclusion_disagreement() -> Result<(), Box<dyn Error>> {
    for (field, value) in [
        ("run_id", json!(40_613_392_799_u64)),
        ("run_attempt", json!(2)),
        ("conclusion", json!("failure")),
    ] {
        let mut record = valid_record("runner");
        record["qualification"][field] = value;
        assert_rejected(&record, field)?;
    }
    Ok(())
}

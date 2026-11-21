// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use serde_json::{Value, json};
use velnor_ci::image_candidate::identity::{buildx_version, workflow};
use velnor_ci::image_candidate::record::{INPUT_PATHS, validate};

pub(super) fn record() -> Value {
    let inputs: serde_json::Map<String, Value> = INPUT_PATHS
        .iter()
        .map(|name| ((*name).to_owned(), json!("b".repeat(64))))
        .collect();
    json!({
        "schema": "velnor.job-image-candidate.v1",
        "source": {"commit": "a".repeat(40), "ref": "refs/heads/candidate", "inputs": inputs},
        "workflow": {"repository": "tailrocks/velnor-new", "repository_id": 1, "event": "push",
            "workflow_ref": "tailrocks/velnor-new/.github/workflows/release.yml@refs/heads/main",
            "authority_commit": "c".repeat(40), "run_id": 10, "run_attempt": 2},
        "platform": "linux/amd64",
        "build": {"builder": "github.com/docker/buildx v0.38.0", "platform": "linux/amd64",
            "arguments": [], "exporter": "oci", "provenance": "disabled",
            "compression": "gzip", "force_compression": true},
        "archive": {"sha256": "d".repeat(64), "size_bytes": 5},
        "oci": {"archive_index_digest": "e".repeat(64), "archive_index_size_bytes": 3,
            "platform_manifest_digest": format!("sha256:{}", "f".repeat(64)),
            "platform_manifest_size_bytes": 3,
            "image_configuration_digest": format!("sha256:{}", "1".repeat(64)),
            "image_configuration_size_bytes": 3,
            "layer_digests": [format!("sha256:{}", "2".repeat(64))]},
    })
}

fn with(path: &[&str], value: Option<Value>) -> Value {
    let mut record = record();
    let mut target = &mut record;
    for key in &path[..path.len() - 1] {
        target = &mut target[*key];
    }
    let last = path[path.len() - 1];
    match value {
        Some(value) => target[last] = value,
        None => {
            target.as_object_mut().map(|map| map.remove(last));
        }
    }
    record
}

fn verdict(record: &Value) -> String {
    validate(record).map_or_else(|e| e.to_string(), |_| "ACCEPTED".to_owned())
}

mod identity;
mod refusals;

type Case = (Vec<&'static str>, Option<Value>, &'static str);

const INPUT: &str = ".velnor/runner.lock";

fn top_and_source_cases() -> Vec<Case> {
    let schema = "Candidate schema or platform is incorrect.";
    let source = "Source identity is incorrect.";
    let inputs = "The source input list is incomplete or unknown.";
    vec![
        (vec!["schema"], Some(json!("v2")), schema),
        (vec!["platform"], Some(json!("linux/arm64")), schema),
        (
            vec!["extra"],
            Some(json!(1)),
            "Candidate record has unknown or missing fields.",
        ),
        (
            vec!["archive"],
            None,
            "Candidate record has unknown or missing fields.",
        ),
        (
            vec!["source", "commit"],
            Some(json!("a".repeat(39))),
            source,
        ),
        (
            vec!["source", "commit"],
            Some(json!("A".repeat(40))),
            source,
        ),
        (vec!["source", "ref"], Some(json!("heads/main")), source),
        (
            vec!["source", "extra"],
            Some(json!(1)),
            "Source has unknown or missing fields.",
        ),
        (vec!["source", "inputs", INPUT], None, inputs),
        (
            vec!["source", "inputs", "extra"],
            Some(json!("b".repeat(64))),
            inputs,
        ),
        (
            vec!["source", "inputs", INPUT],
            Some(json!("b".repeat(63))),
            "Source input hash is incorrect.",
        ),
    ]
}

fn workflow_cases() -> Vec<Case> {
    vec![
        (
            vec!["workflow", "extra"],
            Some(json!(1)),
            "Workflow has unknown or missing fields.",
        ),
        (
            vec!["workflow", "repository"],
            Some(json!("bad")),
            "Workflow repository identity is incorrect.",
        ),
        (
            vec!["workflow", "repository"],
            Some(json!(7)),
            "Workflow repository identity is incorrect.",
        ),
        (
            vec!["workflow", "repository_id"],
            Some(json!(0)),
            "Workflow repository ID is incorrect.",
        ),
        (
            vec!["workflow", "repository_id"],
            Some(json!("1")),
            "Workflow repository ID is incorrect.",
        ),
        (
            vec!["workflow", "event"],
            Some(json!("PUSH")),
            "Workflow event is incorrect.",
        ),
        (
            vec!["workflow", "workflow_ref"],
            Some(json!("a/b/x@refs/heads/main")),
            "Workflow ref is incorrect.",
        ),
        (
            vec!["workflow", "authority_commit"],
            Some(json!("z".repeat(40))),
            "Workflow authority commit is incorrect.",
        ),
        (
            vec!["workflow", "run_id"],
            Some(json!(0)),
            "Workflow run_id is incorrect.",
        ),
        (
            vec!["workflow", "run_attempt"],
            Some(json!(0)),
            "Workflow run_attempt is incorrect.",
        ),
    ]
}

fn build_archive_and_oci_cases() -> Vec<Case> {
    let build = "Build identity is incorrect.";
    let digest = format!("sha256:{}", "A".repeat(64));
    vec![
        (
            vec!["build", "builder"],
            Some(json!("github.com/docker/buildx v0.38")),
            build,
        ),
        (vec!["build", "platform"], Some(json!("linux/arm64")), build),
        (vec!["build", "exporter"], Some(json!("docker")), build),
        (vec!["build", "provenance"], Some(json!("enabled")), build),
        (vec!["build", "compression"], Some(json!("zstd")), build),
        (
            vec!["build", "force_compression"],
            Some(json!(false)),
            build,
        ),
        (
            vec!["build", "arguments"],
            Some(json!(["--push"])),
            "Build arguments must be empty.",
        ),
        (
            vec!["build", "extra"],
            Some(json!(1)),
            "Build has unknown or missing fields.",
        ),
        (
            vec!["archive", "sha256"],
            Some(json!("d".repeat(63))),
            "Archive identity is incorrect.",
        ),
        (
            vec!["archive", "size_bytes"],
            Some(json!(0)),
            "Archive identity is incorrect.",
        ),
        (
            vec!["archive", "extra"],
            Some(json!(1)),
            "Archive has unknown or missing fields.",
        ),
        (
            vec!["oci", "archive_index_digest"],
            Some(json!("e".repeat(63))),
            "OCI archive_index_digest is incorrect.",
        ),
        (
            vec!["oci", "archive_index_size_bytes"],
            Some(json!(0)),
            "OCI archive_index_size_bytes is incorrect.",
        ),
        (
            vec!["oci", "platform_manifest_size_bytes"],
            Some(json!(0)),
            "OCI platform_manifest_size_bytes is incorrect.",
        ),
        (
            vec!["oci", "image_configuration_size_bytes"],
            Some(json!(0)),
            "OCI image_configuration_size_bytes is incorrect.",
        ),
        (
            vec!["oci", "platform_manifest_digest"],
            Some(json!(digest.clone())),
            "OCI platform manifest digest must be a lowercase SHA-256 digest.",
        ),
        (
            vec!["oci", "image_configuration_digest"],
            Some(json!("sha256:short")),
            "OCI image configuration digest must be a lowercase SHA-256 digest.",
        ),
        (
            vec!["oci", "layer_digests"],
            Some(json!([])),
            "OCI layer digest list is incorrect.",
        ),
        (
            vec!["oci", "layer_digests"],
            Some(json!([digest])),
            "OCI layer digest must be a lowercase SHA-256 digest.",
        ),
    ]
}

fn env() -> HashMap<String, String> {
    [
        ("GITHUB_REF", "refs/heads/candidate"),
        ("GITHUB_REPOSITORY", "tailrocks/velnor-new"),
        ("GITHUB_REPOSITORY_ID", "1"),
        ("GITHUB_EVENT_NAME", "push"),
        (
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/release.yml@refs/heads/main",
        ),
        ("GITHUB_WORKFLOW_SHA", &"c".repeat(40)),
        ("GITHUB_RUN_ID", "10"),
        ("GITHUB_RUN_ATTEMPT", "2"),
    ]
    .iter()
    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
    .collect()
}

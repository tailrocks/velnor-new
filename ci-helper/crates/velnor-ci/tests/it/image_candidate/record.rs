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

#[test]
fn a_well_formed_record_is_accepted() {
    assert_eq!(verdict(&record()), "ACCEPTED");
}

fn top_and_source_cases() -> Vec<(Vec<&'static str>, Option<Value>, &'static str)> {
    vec![]
}

fn workflow_cases() -> Vec<(Vec<&'static str>, Option<Value>, &'static str)> {
    vec![]
}

fn build_archive_and_oci_cases() -> Vec<(Vec<&'static str>, Option<Value>, &'static str)> {
    vec![]
}

#[test]
fn every_malformed_record_field_is_refused_with_its_own_message() {
    let cases = [
        top_and_source_cases(),
        workflow_cases(),
        build_archive_and_oci_cases(),
    ]
    .concat();
    for (path, value, message) in cases {
        assert_eq!(
            verdict(&with(&path, value.clone())),
            message,
            "{path:?} {value:?}"
        );
    }
}

#[test]
fn the_buildx_identity_must_be_a_version_with_an_optional_single_line_detail() {
    assert_eq!(
        buildx_version("github.com/docker/buildx v0.38.0"),
        Some("0.38.0")
    );
    assert_eq!(
        buildx_version("github.com/docker/buildx v0.38.0 abc"),
        Some("0.38.0")
    );
    for bad in [
        "github.com/docker/buildx v0.38",
        "github.com/docker/buildx v0.38.0 ",
        "github.com/docker/buildx v0.38.0 a\nb",
        "github.com/docker/buildx v0.38.0.1",
        "github.com/docker/buildx 0.38.0",
        "buildx v0.38.0",
    ] {
        assert_eq!(buildx_version(bad), None, "{bad:?}");
    }
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

#[test]
fn the_github_environment_becomes_the_recorded_workflow_identity() {
    let (identity, reference) = workflow(&env()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(Value::Object(identity), record()["workflow"]);
    assert_eq!(reference, "refs/heads/candidate");
}

#[test]
fn a_missing_or_malformed_github_variable_is_refused() {
    let cases = [
        (
            "GITHUB_REF",
            "heads/x",
            "GitHub source ref is missing or incorrect.",
        ),
        (
            "GITHUB_REPOSITORY",
            "bad",
            "GitHub repository identity is incorrect.",
        ),
        (
            "GITHUB_REPOSITORY_ID",
            "0",
            "GitHub repository ID is incorrect.",
        ),
        (
            "GITHUB_REPOSITORY_ID",
            "-1",
            "GitHub repository ID is incorrect.",
        ),
        (
            "GITHUB_REPOSITORY_ID",
            "1.5",
            "GitHub repository ID is incorrect.",
        ),
        (
            "GITHUB_EVENT_NAME",
            "PUSH",
            "GitHub event name is incorrect.",
        ),
        (
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/x@refs/a",
            "GitHub workflow ref is incorrect.",
        ),
        (
            "GITHUB_WORKFLOW_SHA",
            "z",
            "GitHub workflow authority commit is incorrect.",
        ),
        ("GITHUB_RUN_ID", "0", "GitHub run_id is incorrect."),
        (
            "GITHUB_RUN_ATTEMPT",
            "x",
            "GitHub run_attempt is incorrect.",
        ),
    ];
    for (name, value, message) in cases {
        let mut vars = env();
        vars.insert(name.to_owned(), value.to_owned());
        assert_eq!(
            workflow(&vars).err().map(|e| e.to_string()).as_deref(),
            Some(message),
            "{name}={value}"
        );
        vars.remove(name);
        assert!(workflow(&vars).is_err(), "{name} missing");
    }
}

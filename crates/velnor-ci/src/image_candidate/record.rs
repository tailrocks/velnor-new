// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Map, Value};

use super::identity::{buildx_version, is_event, is_repository, is_workflow_ref};
use super::json::{is_digest, is_hex, is_int_at_least};
use crate::error::CiError;

pub const SCHEMA: &str = "velnor.job-image-candidate.v1";
pub const INPUT_PATHS: &[&str] = &[
    ".velnor/runner.lock",
    "images/job/ubuntu-26.04/Dockerfile",
    "images/job/ubuntu-26.04/README.md",
    "images/job/ubuntu-26.04/build-test.sh",
    "images/job/ubuntu-26.04/candidate/candidate.py",
];

fn fail<T>(message: impl Into<String>) -> Result<T, CiError> {
    Err(CiError::new(message))
}

fn exact<'a>(
    value: &'a Value,
    keys: &[&str],
    label: &str,
) -> Result<&'a Map<String, Value>, CiError> {
    match value.as_object() {
        Some(map) if map.len() == keys.len() && keys.iter().all(|key| map.contains_key(*key)) => {
            Ok(map)
        }
        _ => fail(format!("{label} has unknown or missing fields.")),
    }
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn hex_text(value: &Value, length: usize) -> bool {
    value.as_str().is_some_and(|text| is_hex(text, length))
}

pub fn validate(value: &Value) -> Result<&Map<String, Value>, CiError> {
    let record = exact(
        value,
        &[
            "schema", "source", "workflow", "platform", "build", "archive", "oci",
        ],
        "Candidate record",
    )?;
    if record["schema"] != SCHEMA || record["platform"] != "linux/amd64" {
        return fail("Candidate schema or platform is incorrect.");
    }
    source(&record["source"])?;
    workflow(&record["workflow"])?;
    build(&record["build"])?;
    let archive = exact(&record["archive"], &["sha256", "size_bytes"], "Archive")?;
    if !hex_text(&archive["sha256"], 64) || !is_int_at_least(&archive["size_bytes"], 1) {
        return fail("Archive identity is incorrect.");
    }
    oci(&record["oci"])?;
    Ok(record)
}

fn source(value: &Value) -> Result<(), CiError> {
    let source = exact(value, &["commit", "ref", "inputs"], "Source")?;
    if !hex_text(&source["commit"], 40) || !text(&source["ref"]).starts_with("refs/") {
        return fail("Source identity is incorrect.");
    }
    let inputs = source["inputs"].as_object();
    if inputs.is_none_or(|map| {
        map.len() != INPUT_PATHS.len() || !INPUT_PATHS.iter().all(|name| map.contains_key(*name))
    }) {
        return fail("The source input list is incomplete or unknown.");
    }
    if inputs.is_some_and(|map| map.values().any(|digest| !hex_text(digest, 64))) {
        return fail("Source input hash is incorrect.");
    }
    Ok(())
}

fn workflow(value: &Value) -> Result<(), CiError> {
    let keys = [
        "repository",
        "repository_id",
        "event",
        "workflow_ref",
        "authority_commit",
        "run_id",
        "run_attempt",
    ];
    let workflow = exact(value, &keys, "Workflow")?;
    let repository = text(&workflow["repository"]);
    if !workflow["repository"].is_string() || !is_repository(repository) {
        return fail("Workflow repository identity is incorrect.");
    }
    if !is_int_at_least(&workflow["repository_id"], 1) {
        return fail("Workflow repository ID is incorrect.");
    }
    if !workflow["event"].is_string() || !is_event(text(&workflow["event"])) {
        return fail("Workflow event is incorrect.");
    }
    if !workflow["workflow_ref"].is_string()
        || !is_workflow_ref(repository, text(&workflow["workflow_ref"]))
    {
        return fail("Workflow ref is incorrect.");
    }
    if !hex_text(&workflow["authority_commit"], 40) {
        return fail("Workflow authority commit is incorrect.");
    }
    for name in ["run_id", "run_attempt"] {
        if !is_int_at_least(&workflow[name], 1) {
            return fail(format!("Workflow {name} is incorrect."));
        }
    }
    Ok(())
}

fn build(value: &Value) -> Result<(), CiError> {
    let keys = [
        "builder",
        "platform",
        "arguments",
        "exporter",
        "provenance",
        "compression",
        "force_compression",
    ];
    let build = exact(value, &keys, "Build")?;
    let builder_ok = build["builder"].as_str().and_then(buildx_version).is_some();
    if !builder_ok
        || build["platform"] != "linux/amd64"
        || build["exporter"] != "oci"
        || build["provenance"] != "disabled"
        || build["compression"] != "gzip"
        || build["force_compression"] != Value::Bool(true)
    {
        return fail("Build identity is incorrect.");
    }
    if build["arguments"] != Value::Array(Vec::new()) {
        return fail("Build arguments must be empty.");
    }
    Ok(())
}

fn oci(value: &Value) -> Result<(), CiError> {
    let keys = [
        "archive_index_digest",
        "archive_index_size_bytes",
        "platform_manifest_digest",
        "platform_manifest_size_bytes",
        "image_configuration_digest",
        "image_configuration_size_bytes",
        "layer_digests",
    ];
    let oci = exact(value, &keys, "OCI")?;
    if !hex_text(&oci["archive_index_digest"], 64) {
        return fail("OCI archive_index_digest is incorrect.");
    }
    for name in [
        "archive_index_size_bytes",
        "platform_manifest_size_bytes",
        "image_configuration_size_bytes",
    ] {
        if !is_int_at_least(&oci[name], 1) {
            return fail(format!("OCI {name} is incorrect."));
        }
    }
    if !is_digest(&oci["platform_manifest_digest"]) {
        return fail("OCI platform manifest digest must be a lowercase SHA-256 digest.");
    }
    if !is_digest(&oci["image_configuration_digest"]) {
        return fail("OCI image configuration digest must be a lowercase SHA-256 digest.");
    }
    let layers = oci["layer_digests"]
        .as_array()
        .filter(|layers| !layers.is_empty());
    let Some(layers) = layers else {
        return fail("OCI layer digest list is incorrect.");
    };
    if layers.iter().any(|digest| !is_digest(digest)) {
        return fail("OCI layer digest must be a lowercase SHA-256 digest.");
    }
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use base64::Engine as _;
use serde_json::{Map, Value, json};

use super::json::{Field, Res, hex64, is_semver, require, sha40};
use super::net::{Net, repo_path};
use super::{
    CI_WORKFLOW, DARWIN_TARGET, MAX_LOCK, MAX_PAYLOAD, QUALIFICATION_WORKFLOW, REPOSITORY,
    SIGNER_WORKFLOW, TARGET,
};
use crate::error::CiError;

const RUNS: [&str; 5] = ["source", "draft", "signer", "ci", "qualification"];
const RUN_FIELDS: [&str; 5] = [
    "workflow_path",
    "workflow_ref",
    "workflow_sha",
    "run_id",
    "run_attempt",
];
const TARGET_FIELDS: [&str; 10] = [
    "producer_run_id",
    "producer_run_attempt",
    "producer_payload_artifact_id",
    "producer_payload_archive_sha256",
    "candidate_payload_artifact_id",
    "qualification_job_id",
    "payload_target",
    "payload_name",
    "payload_size",
    "payload_sha256",
];
const FIXED: [&str; 11] = [
    "schema",
    "repository",
    "version",
    "source_sha",
    "tag",
    "release_id",
    "receipt_sha256",
    "bundle_sha256",
    "candidate_source_artifact_id",
    "publisher_run_attempt",
    "publisher_job_id",
];

fn expected_keys() -> BTreeSet<String> {
    let mut keys: BTreeSet<String> = FIXED.iter().map(|k| (*k).to_owned()).collect();
    for run in RUNS {
        keys.extend(RUN_FIELDS.iter().map(|f| format!("{run}_{f}")));
    }
    for target in ["linux", "darwin"] {
        keys.extend(TARGET_FIELDS.iter().map(|f| format!("{target}_{f}")));
    }
    keys
}

fn workflow_tuple(
    record: &Value,
    prefix: &str,
    path: &str,
    source: &str,
    git_ref: &str,
) -> Res<()> {
    require(record.at(&format!("{prefix}_workflow_path")).is_s(path))?;
    require(
        record
            .at(&format!("{prefix}_workflow_ref"))
            .is_s(&format!("{REPOSITORY}/{path}@{git_ref}")),
    )?;
    require(record.at(&format!("{prefix}_workflow_sha")).is_s(source))?;
    require(record.at(&format!("{prefix}_run_id")).is_pos())?;
    require(record.at(&format!("{prefix}_run_attempt")).is_pos())
}

fn attempt(record: &Value, key: &str) -> Res<u64> {
    record
        .at(key)
        .as_u64()
        .ok_or_else(|| CiError::new("source preview attempt"))
}

/// Port of `validate_preview` of `source_preview_bootstrap.py`.
pub fn validate_preview(record: &Value, candidate: &str) -> Res<()> {
    let map = record
        .as_object()
        .ok_or_else(|| CiError::new("source preview record"))?;
    require(map.keys().cloned().collect::<BTreeSet<_>>() == expected_keys())?;
    let source = record.at("source_sha").text();
    require(record.at("schema").as_i64() == Some(3) && record.at("repository").is_s(REPOSITORY))?;
    require(is_semver(record.at("version").text()))?;
    require(
        sha40(record.at("source_sha")) && super::json::is_hex(candidate, 40) && source != candidate,
    )?;
    let tag = format!("sdk-preview-{source}");
    require(record.at("tag").is_s(&tag) && record.at("release_id").is_pos())?;
    let tag_ref = format!("refs/tags/{tag}");
    for role in ["source", "draft"] {
        workflow_tuple(record, role, SIGNER_WORKFLOW, source, &tag_ref)?;
        require(record.at(&format!("{role}_run_id")) == record.at("signer_run_id"))?;
    }
    for key in [
        "candidate_source_artifact_id",
        "publisher_run_attempt",
        "publisher_job_id",
    ] {
        require(record.at(key).is_pos())?;
    }
    validate_targets(record)?;
    let attempts = [
        attempt(record, "source_run_attempt")?,
        attempt(record, "draft_run_attempt")?,
        attempt(record, "signer_run_attempt")?,
        attempt(record, "publisher_run_attempt")?,
    ];
    require(attempts.windows(2).all(|w| w[0] <= w[1]) && attempts[3] <= 51)?;
    workflow_tuple(record, "signer", SIGNER_WORKFLOW, source, &tag_ref)?;
    workflow_tuple(record, "ci", CI_WORKFLOW, source, "refs/heads/main")?;
    workflow_tuple(
        record,
        "qualification",
        QUALIFICATION_WORKFLOW,
        source,
        "refs/heads/main",
    )?;
    require(hex64(record.at("receipt_sha256")) && hex64(record.at("bundle_sha256")))
}

fn validate_targets(record: &Value) -> Res<()> {
    let mut artifacts = vec![record.at("candidate_source_artifact_id").clone()];
    let mut jobs = Vec::new();
    for (prefix, target) in [("linux", TARGET), ("darwin", DARWIN_TARGET)] {
        let key = |name: &str| record.at(&format!("{prefix}_{name}"));
        for name in [
            "producer_run_id",
            "producer_run_attempt",
            "producer_payload_artifact_id",
            "candidate_payload_artifact_id",
            "qualification_job_id",
            "payload_size",
        ] {
            require(key(name).is_pos())?;
        }
        require(
            key("payload_size")
                .as_u64()
                .is_some_and(|n| n <= MAX_PAYLOAD as u64),
        )?;
        require(key("payload_target").is_s(target))?;
        require(key("payload_name").is_s(&format!("velnor-actions-{target}")))?;
        require(hex64(key("producer_payload_archive_sha256")) && hex64(key("payload_sha256")))?;
        artifacts.push(key("producer_payload_artifact_id").clone());
        artifacts.push(key("candidate_payload_artifact_id").clone());
        jobs.push(key("qualification_job_id").clone());
    }
    let unique: BTreeSet<String> = artifacts.iter().map(Value::to_string).collect();
    require(unique.len() == artifacts.len() && jobs[0] != jobs[1])
}

/// Port of `receipt_from_record`: the receipt the record claims the signer wrote.
#[must_use]
pub fn receipt_from_record(record: &Value) -> Value {
    let run = |prefix: &str| -> Value {
        let map: Map<String, Value> = RUN_FIELDS
            .iter()
            .map(|f| ((*f).to_owned(), record.at(&format!("{prefix}_{f}")).clone()))
            .collect();
        Value::Object(map)
    };
    let target = |prefix: &str| -> Value {
        let field = |name: &str| record.at(&format!("{prefix}_{name}")).clone();
        json!({
            "producer": {
                "producer_run_id": field("producer_run_id"),
                "producer_run_attempt": field("producer_run_attempt"),
                "payload_artifact_id": field("producer_payload_artifact_id"),
                "payload_archive_sha256": field("producer_payload_archive_sha256"),
            },
            "candidate_payload_artifact_id": field("candidate_payload_artifact_id"),
            "qualification_job_id": field("qualification_job_id"),
            "payload": {
                "target": field("payload_target"),
                "name": field("payload_name"),
                "size": field("payload_size"),
                "sha256": field("payload_sha256"),
            },
        })
    };
    json!({
        "schema": record.at("schema"),
        "repository": record.at("repository"),
        "version": record.at("version"),
        "source_sha": record.at("source_sha"),
        "tag": record.at("tag"),
        "release_id": record.at("release_id"),
        "source": run("source"),
        "draft": run("draft"),
        "signer": run("signer"),
        "ci": run("ci"),
        "qualification": run("qualification"),
        "candidate_source_artifact_id": record.at("candidate_source_artifact_id"),
        "targets": {"linux": target("linux"), "darwin": target("darwin")},
    })
}

/// Parse `.velnor/generator.lock` as TOML into a JSON value.
pub fn parse_lock(data: &[u8]) -> Res<Value> {
    require(data.len() <= MAX_LOCK)?;
    let text =
        std::str::from_utf8(data).map_err(|_| CiError::new("source preview invalid lock"))?;
    toml::from_str::<Value>(text).map_err(|_| CiError::new("source preview invalid lock"))
}

/// Port of `read_github_lock`: the lock at `commit`, read through the contents API.
pub fn read_github_lock(net: &dyn Net, commit: &str) -> Res<Value> {
    require(super::json::is_hex(commit, 40))?;
    let blob = net.api(&repo_path(&format!(
        "/contents/.velnor/generator.lock?ref={commit}"
    )))?;
    require(blob.at("type").is_s("file") && blob.at("encoding").is_s("base64"))?;
    require(blob.at("path").is_s(".velnor/generator.lock") && blob.at("size").is_pos())?;
    let size = blob.at("size").as_u64().unwrap_or(0);
    require(size <= MAX_LOCK as u64)?;
    let joined: String = blob.at("content").text().split_whitespace().collect();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(joined)
        .map_err(|_| CiError::new("source preview invalid base lock"))?;
    require(raw.len() as u64 == size)?;
    parse_lock(&raw)
}

/// Port of `approved_preview`: the candidate lock's preview, only if the base lock
/// already approved or staged the same record.
pub fn approved_preview(
    candidate_lock: &Value,
    base_lock: &Value,
    role: &str,
    candidate: &str,
) -> Res<Option<Value>> {
    require(role == "planner" || role == "runtime")?;
    let active = format!("{role}-preview");
    let preview = candidate_lock.at(&active);
    let base_active = base_lock.at(&active);
    if preview.is_null() {
        require(base_active.is_null())?;
        return Ok(None);
    }
    validate_preview(preview, candidate)?;
    let staged = base_lock.at(&format!("{role}-preview-staged"));
    require(
        (!base_active.is_null() && base_active == preview)
            || (!staged.is_null() && staged == preview),
    )?;
    Ok(Some(preview.clone()))
}

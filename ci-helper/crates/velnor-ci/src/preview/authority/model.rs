// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use crate::error::CiError;
use crate::preview::json::{Field, Res, canonical, digest, is_hex, is_semver, parse, require};
use crate::preview::{
    CI_WORKFLOW, DARWIN_TARGET, QUALIFICATION_WORKFLOW, REPOSITORY, SIGNER_WORKFLOW, TARGET,
};

pub const RECEIPT: &str = "SDK_PREVIEW_RECEIPT.json";
pub const BUNDLE: &str = "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl";
pub const MARKER_PREFIX: &str = "velnor-source-preview:";
pub const TARGETS: [(&str, &str, &str); 2] = [
    ("linux", TARGET, "velnor-actions-x86_64-unknown-linux-gnu"),
    (
        "darwin",
        DARWIN_TARGET,
        "velnor-actions-aarch64-apple-darwin",
    ),
];
pub const PAYLOAD_NAMES: [&str; 2] = [TARGETS[0].2, TARGETS[1].2];
const MAX_SAFE: u64 = 9_007_199_254_740_991;

/// Port of `positive`: an integer from 1 to 2^53 - 1.
pub fn positive(value: &Value) -> Res<u64> {
    value
        .as_u64()
        .filter(|n| *n > 0 && *n <= MAX_SAFE)
        .ok_or_else(|| CiError::new("source preview number"))
}

/// A decimal environment value, as `positive(int(...))`.
pub fn positive_text(text: &str) -> Res<u64> {
    text.trim()
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && *n <= MAX_SAFE)
        .ok_or_else(|| CiError::new("source preview number"))
}

#[must_use]
pub fn asset_names() -> BTreeSet<&'static str> {
    let mut names: BTreeSet<&str> = PAYLOAD_NAMES.into_iter().collect();
    names.extend([RECEIPT, BUNDLE]);
    names
}

/// Port of `candidate_marker`: the text a draft release carries in its body, which
/// binds the release to its source metadata and the artifacts of one run.
pub fn candidate_marker(source: &Value, candidate: &Value) -> Res<String> {
    let keys: BTreeSet<&str> = candidate
        .as_object()
        .map_or_else(BTreeSet::new, |m| m.keys().map(String::as_str).collect());
    require(
        keys == BTreeSet::from([
            "source_artifact_id",
            "linux_payload_artifact_id",
            "darwin_payload_artifact_id",
        ]),
    )?;
    for value in candidate.as_object().into_iter().flat_map(Map::values) {
        positive(value)?;
    }
    positive(source.at("source").at("run_id"))?;
    let marker = json!({
        "schema": 3,
        "source_sha": source.at("source_sha"),
        "run_id": source.at("source").at("run_id"),
        "targets": source.at("targets"),
        "candidate": candidate,
    });
    Ok(format!(
        "{MARKER_PREFIX}{}",
        String::from_utf8_lossy(&canonical(&marker))
    ))
}

/// Port of `payload_identity`: the payload is an ELF x86-64 or Mach-O arm64 file.
pub fn payload_identity(key: &str, data: &[u8]) -> Res<Value> {
    let (_, target, name) = TARGETS
        .iter()
        .find(|(k, ..)| *k == key)
        .ok_or_else(|| CiError::new("source preview target"))?;
    require(data.len() >= 20)?;
    if key == "linux" {
        require(data[..7] == *b"\x7fELF\x02\x01\x01" && data[18..20] == *b"\x3e\x00")?;
    } else {
        require(data.len() >= 32 && data[..4] == *b"\xcf\xfa\xed\xfe")?;
        require(data[4..8] == *b"\x0c\x00\x00\x01" && data[12..16] == *b"\x02\x00\x00\x00")?;
    }
    Ok(json!({"target": target, "name": name, "size": data.len(), "sha256": digest(data)}))
}

/// Port of `receipt_targets`.
pub fn receipt_targets(
    source: &Value,
    candidate: &Value,
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Res<Value> {
    require(payloads.len() == 2)?;
    let mut targets = Map::new();
    for (key, ..) in TARGETS {
        let data = payloads
            .get(key)
            .ok_or_else(|| CiError::new("source preview payloads"))?;
        let from = source.at("targets").at(key);
        targets.insert(
            key.to_owned(),
            json!({
                "producer": from.at("producer"),
                "qualification_job_id": from.at("qualification_job_id"),
                "candidate_payload_artifact_id": candidate.at(&format!("{key}_payload_artifact_id")),
                "payload": payload_identity(key, data)?,
            }),
        );
    }
    Ok(Value::Object(targets))
}

/// Port of `finalize_receipt`: the canonical receipt the attest job signs.
pub fn finalize_receipt(decision: &Value, attempt: u64) -> Res<Vec<u8>> {
    let keys = ["source", "candidate", "release_id", "targets", "draft"];
    require(
        decision
            .as_object()
            .is_some_and(|m| m.len() == 5 && keys.iter().all(|k| m.contains_key(*k))),
    )?;
    let source = decision.at("source");
    let draft = decision.at("draft");
    let mut signer = draft.clone();
    signer["run_attempt"] = json!(attempt);
    require(attempt > 0 && source.at("source").at("run_id") == draft.at("run_id"))?;
    for key in ["workflow_path", "workflow_ref", "workflow_sha"] {
        require(source.at("source").at(key) == draft.at(key))?;
    }
    let (a, b) = (
        source.at("source").at("run_attempt").as_u64(),
        draft.at("run_attempt").as_u64(),
    );
    require(a.zip(b).is_some_and(|(a, b)| a <= b && b <= attempt))?;
    candidate_marker(source, decision.at("candidate"))?;
    let mut receipt: Map<String, Value> = source
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(k, _)| k.as_str() != "targets")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    receipt.insert(
        "candidate_source_artifact_id".to_owned(),
        decision.at("candidate").at("source_artifact_id").clone(),
    );
    receipt.insert(
        "release_id".to_owned(),
        json!(positive(decision.at("release_id"))?),
    );
    receipt.insert("targets".to_owned(), decision.at("targets").clone());
    receipt.insert("draft".to_owned(), draft.clone());
    receipt.insert("signer".to_owned(), signer);
    Ok(canonical(&Value::Object(receipt)))
}

/// The environment `metadata` checks the source metadata against.
#[derive(Debug, Clone)]
pub struct Run<'a> {
    pub sha: &'a str,
    pub repository: &'a str,
    pub event_name: &'a str,
    pub git_ref: &'a str,
    pub workflow_ref: &'a str,
    pub workflow_sha: &'a str,
    pub run_id: u64,
    pub run_attempt: u64,
}

/// Port of `metadata`: the admitted source metadata is canonical, and names this
/// run's own tag push.
pub fn metadata(data: &[u8], run: &Run<'_>) -> Res<Value> {
    let value = parse(data, usize::MAX)?;
    let tag = format!("sdk-preview-{}", run.sha);
    let tag_ref = format!("refs/tags/{tag}");
    require(canonical(&value) == data && is_hex(run.sha, 40))?;
    require(run.repository == REPOSITORY && run.event_name == "push" && run.git_ref == tag_ref)?;
    require(
        run.workflow_ref == format!("{REPOSITORY}/{SIGNER_WORKFLOW}@{tag_ref}")
            && run.workflow_sha == run.sha,
    )?;
    let keys: BTreeSet<&str> = value
        .as_object()
        .map_or_else(BTreeSet::new, |m| m.keys().map(String::as_str).collect());
    require(
        keys == BTreeSet::from([
            "schema",
            "repository",
            "version",
            "source_sha",
            "tag",
            "source",
            "ci",
            "qualification",
            "targets",
        ]),
    )?;
    require(value.at("schema").as_i64() == Some(3) && value.at("repository").is_s(REPOSITORY))?;
    require(value.at("source_sha").is_s(run.sha) && value.at("tag").is_s(&tag))?;
    require(is_semver(value.at("version").text()))?;
    for (key, path) in [
        ("source", SIGNER_WORKFLOW),
        ("ci", CI_WORKFLOW),
        ("qualification", QUALIFICATION_WORKFLOW),
    ] {
        let item = value.at(key);
        let fields = [
            "workflow_path",
            "workflow_ref",
            "workflow_sha",
            "run_id",
            "run_attempt",
        ];
        require(
            item.as_object()
                .is_some_and(|m| m.len() == 5 && fields.iter().all(|f| m.contains_key(*f))),
        )?;
        let git_ref = if key == "source" {
            tag_ref.as_str()
        } else {
            "refs/heads/main"
        };
        require(item.at("workflow_path").is_s(path))?;
        require(
            item.at("workflow_ref")
                .is_s(&format!("{REPOSITORY}/{path}@{git_ref}")),
        )?;
        require(item.at("workflow_sha").is_s(run.sha))?;
        positive(item.at("run_id"))?;
        positive(item.at("run_attempt"))?;
    }
    let attempt = value
        .at("source")
        .at("run_attempt")
        .as_u64()
        .unwrap_or(u64::MAX);
    require(
        value.at("source").at("run_id").is_u(run.run_id)
            && attempt <= run.run_attempt
            && run.run_attempt <= 51,
    )?;
    targets(value.at("targets"))?;
    Ok(value)
}

fn targets(value: &Value) -> Res<()> {
    let keys: BTreeSet<&str> = value
        .as_object()
        .map_or_else(BTreeSet::new, |m| m.keys().map(String::as_str).collect());
    require(keys == BTreeSet::from(["linux", "darwin"]))?;
    for target in value.as_object().into_iter().flat_map(Map::values) {
        let outer: BTreeSet<&str> = target
            .as_object()
            .map_or_else(BTreeSet::new, |m| m.keys().map(String::as_str).collect());
        require(outer == BTreeSet::from(["producer", "qualification_job_id"]))?;
        positive(target.at("qualification_job_id"))?;
        let producer = target.at("producer");
        let inner: BTreeSet<&str> = producer
            .as_object()
            .map_or_else(BTreeSet::new, |m| m.keys().map(String::as_str).collect());
        require(
            inner
                == BTreeSet::from([
                    "producer_run_id",
                    "producer_run_attempt",
                    "payload_artifact_id",
                    "payload_archive_sha256",
                ]),
        )?;
        for key in [
            "producer_run_id",
            "producer_run_attempt",
            "payload_artifact_id",
        ] {
            positive(producer.at(key))?;
        }
        require(crate::preview::json::hex64(
            producer.at("payload_archive_sha256"),
        ))?;
    }
    Ok(())
}

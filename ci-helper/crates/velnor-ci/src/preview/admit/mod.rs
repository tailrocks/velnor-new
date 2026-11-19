// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `admit-preview`: the "Admit source preview" job of `source-preview-publication.yml`.
//! A preview is admitted only when the run is the owner's push of the annotated tag
//! `sdk-preview-<commit>`, made under protected tag rules, for a commit on `main` whose
//! qualification runs and both payload artifacts succeeded and still are what the tag
//! says. Then both payloads are downloaded and checked, and the metadata the later jobs
//! rely on is written. This module is the pure part, which reads GitHub through
//! [`Client`]; `crate::io::preview::admit` writes the files.

pub mod clock;
pub mod runs;
pub mod tag;

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use super::authority::Client;
use super::authority::github::artifact_files;
use super::authority::model::{TARGETS, payload_identity, positive, positive_text};
use super::authority::modes::Inputs;
use super::json::{Field, Res, require};
use super::{MAX_PAYLOAD, REPOSITORY, REPOSITORY_ID, SIGNER_WORKFLOW};
use crate::error::CiError;

/// The only account that may push a preview tag.
pub const ACTOR_ID: u64 = 139_017;
pub const ACTOR_LOGIN: &str = "donbeave";
pub const ACTOR_PATH: &str = "/users/donbeave";
const REPOSITORY_OWNER_ID: u64 = 78_806_509;

/// What the job admitted: the metadata for the later jobs, and both payloads.
#[derive(Debug)]
pub struct Admission {
    pub metadata: Value,
    pub payloads: Vec<(&'static str, Vec<u8>)>,
}

/// A decimal environment value, as `positive(int(...))`.
pub fn decimal(inputs: &Inputs, key: &str) -> Res<u64> {
    positive_text(inputs.req(key)?)
}

/// Port of `verify_repository`.
fn verify_repository(client: &dyn Client) -> Res<()> {
    let repository = client.get("")?;
    require(
        repository.at("id").is_u(REPOSITORY_ID)
            && repository.at("full_name").is_s(REPOSITORY)
            && repository.at("default_branch").is_s("main")
            && repository.at("owner").at("id").is_u(REPOSITORY_OWNER_ID),
    )
}

fn semver(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            p.bytes().all(|b| b.is_ascii_digit())
                && (*p == "0" || (!p.is_empty() && !p.starts_with('0')))
        })
}

/// Port of `source_version`: `workspace.package.version` of `Cargo.toml` at `source`.
fn source_version(client: &dyn Client, source: &str) -> Res<String> {
    let blob = client.get(&format!("/contents/Cargo.toml?ref={source}"))?;
    require(blob.at("path").is_s("Cargo.toml") && blob.at("encoding").is_s("base64"))?;
    let letters: Vec<u8> = blob
        .at("content")
        .text()
        .bytes()
        .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
        .collect();
    let bytes = STANDARD
        .decode(letters)
        .map_err(|_| CiError::new("source preview version source"))?;
    let manifest: toml::Table = toml::from_str(
        std::str::from_utf8(&bytes).map_err(|_| CiError::new("source preview version source"))?,
    )
    .map_err(|_| CiError::new("source preview version source"))?;
    let version = manifest
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| CiError::new("source preview release version"))?;
    require(semver(version))?;
    Ok(version.to_owned())
}

/// The metadata of the admitted preview, from the tag's annotation and the qualification runs.
fn metadata(proof: &tag::Proof, source: &str, version: &str, run: (u64, u64)) -> Value {
    let targets: BTreeMap<&str, Value> = TARGETS
        .iter()
        .map(|(key, ..)| {
            let item = proof.record.at("targets").at(key);
            let producer: BTreeMap<&str, &Value> = [
                "producer_run_id",
                "producer_run_attempt",
                "payload_artifact_id",
                "payload_archive_sha256",
            ]
            .into_iter()
            .map(|name| (name, item.at(name)))
            .collect();
            (
                *key,
                json!({"producer": producer, "qualification_job_id": item.at("qualification_job_id")}),
            )
        })
        .collect();
    json!({
        "schema": 3, "repository": REPOSITORY, "version": version, "source_sha": source,
        "tag": format!("sdk-preview-{source}"), "targets": targets,
        "source": {"workflow_path": SIGNER_WORKFLOW, "workflow_ref": proof.reference,
                   "workflow_sha": source, "run_id": run.0, "run_attempt": run.1},
    })
}

/// Port of `admit`.
pub fn admit(client: &dyn Client, inputs: &Inputs) -> Res<Admission> {
    inputs.authority()?;
    let source = inputs.req("GITHUB_SHA")?;
    require(super::json::is_hex(source, 40))?;
    verify_repository(client)?;
    let version = source_version(client, source)?;
    let proof = tag::admit_tag(client, inputs, source, &version)?;
    let run = (
        decimal(inputs, "GITHUB_RUN_ID")?,
        decimal(inputs, "GITHUB_RUN_ATTEMPT")?,
    );
    let mut metadata = metadata(&proof, source, &version, run);
    let (ci, qualification) = runs::qualification_proofs(client, source)?;
    metadata["ci"] = ci;
    metadata["qualification"] = qualification;
    let mut payloads = Vec::new();
    for (key, triple, name) in TARGETS {
        let item = proof.record.at("targets").at(key);
        let (artifact, job) = runs::producer_artifact(
            client,
            source,
            positive(item.at("producer_run_id"))?,
            positive(item.at("producer_run_attempt"))?,
            key,
        )?;
        require(
            artifact.at("id") == item.at("payload_artifact_id")
                && artifact.at("digest").is_s(&format!(
                    "sha256:{}",
                    item.at("payload_archive_sha256").text()
                ))
                && job == positive(item.at("qualification_job_id"))?,
        )?;
        payloads.push((key, triple, name, artifact));
    }
    let mut admitted = Vec::new();
    for (key, triple, name, artifact) in &payloads {
        let mut files = artifact_files(client, artifact, &BTreeMap::from([(*name, MAX_PAYLOAD)]))?;
        let data = files
            .remove(*name)
            .ok_or_else(|| CiError::new("source preview payload"))?;
        payload_identity(key, &data)?;
        admitted.push((*triple, data));
    }
    require(tag::tag_reference(client, source)? == proof.tag_sha)?;
    tag::verify_default_ancestry(client, source)?;
    Ok(Admission {
        metadata,
        payloads: admitted,
    })
}

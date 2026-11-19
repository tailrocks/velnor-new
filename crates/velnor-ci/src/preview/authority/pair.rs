// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::Client;
use super::github::{inventory, released_pair, successful_job};
use super::model::{BUNDLE, RECEIPT, Run, TARGETS, candidate_marker, positive, receipt_targets};
use crate::error::CiError;
use crate::preview::attest::{Attestor, Authority, Subject};
use crate::preview::json::{Field, Res, canonical, digest, parse, require};
use crate::preview::{MAX_JSON, SIGNER_WORKFLOW};

/// Everything a check needs besides the data it checks.
#[derive(Debug)]
pub struct Ctx<'a> {
    pub client: &'a dyn Client,
    pub attestor: &'a dyn Attestor,
    pub run: Run<'a>,
    pub authority: &'a Authority,
}

/// A signed receipt and bundle found for the release, and where it was found.
#[derive(Debug, Clone)]
pub struct Pair {
    pub receipt: Vec<u8>,
    pub bundle: Vec<u8>,
    pub signer_attempt: u64,
    pub origin: &'static str,
    pub artifact_id: Option<u64>,
}

/// The outcome of `prepare_pair`: sign afresh, or reuse a pair already signed.
#[derive(Debug)]
pub enum Selection {
    Sign,
    Reuse(Pair),
}

const ROLES: [(&str, &str); 3] = [
    (
        "source",
        "Source preview publication / Admit source preview",
    ),
    ("draft", "Source preview publication / Draft source preview"),
    (
        "signer",
        "Source preview publication / Attest source preview",
    ),
];

fn keys_of(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .map_or_else(BTreeSet::new, |m| m.keys().map(String::as_str).collect())
}

/// What a receipt must say about the source, the candidate, the payloads and its two runs.
fn check_receipt(
    receipt: &Value,
    source: &Value,
    candidate: &Value,
    release_id: u64,
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Res<()> {
    let mut want = keys_of(source);
    want.remove("targets");
    want.extend([
        "release_id",
        "candidate_source_artifact_id",
        "targets",
        "draft",
        "signer",
    ]);
    require(keys_of(receipt) == want)?;
    for (key, value) in source.as_object().into_iter().flatten() {
        if key != "targets" {
            require(receipt.at(key) == value)?;
        }
    }
    require(receipt.at("candidate_source_artifact_id") == candidate.at("source_artifact_id"))?;
    require(receipt.at("release_id").is_u(release_id))?;
    require(receipt.at("targets") == &receipt_targets(source, candidate, payloads)?)?;
    let origin = source.at("source");
    for role in ["draft", "signer"] {
        let run = receipt.at(role);
        require(keys_of(run) == keys_of(origin))?;
        for key in ["workflow_path", "workflow_ref", "workflow_sha", "run_id"] {
            require(run.at(key) == origin.at(key))?;
        }
        positive(run.at("run_attempt"))?;
    }
    Ok(())
}

/// Port of `verify_pair`: the receipt is canonical, says what the source, candidate and
/// payloads say, comes from successful runs, and carries a valid attestation.
pub fn verify_pair(
    ctx: &Ctx<'_>,
    source: &Value,
    candidate: &Value,
    release_id: u64,
    payloads: &BTreeMap<String, Vec<u8>>,
    receipt_bytes: &[u8],
    bundle: &[u8],
) -> Res<Pair> {
    let receipt = parse(receipt_bytes, usize::MAX)?;
    require(canonical(&receipt) == receipt_bytes)?;
    check_receipt(&receipt, source, candidate, release_id, payloads)?;
    let origin = source.at("source");
    let attempts = [
        origin.at("run_attempt").as_u64(),
        receipt.at("draft").at("run_attempt").as_u64(),
        receipt.at("signer").at("run_attempt").as_u64(),
        Some(ctx.run.run_attempt),
    ];
    require(attempts.iter().all(Option::is_some))?;
    require(attempts.windows(2).all(|w| w[0] <= w[1]) && ctx.run.run_attempt <= 51)?;
    let source_sha = source.at("source_sha").text();
    for (role, name) in ROLES {
        let attempt = receipt.at(role).at("run_attempt").as_u64().unwrap_or(0);
        successful_job(
            ctx.client,
            source_sha,
            positive(origin.at("run_id"))?,
            attempt,
            name,
            false,
        )?;
    }
    let target = |key: &str, field: &str| {
        receipt
            .at("targets")
            .at(key)
            .at("payload")
            .at(field)
            .text()
            .to_owned()
    };
    let subject = Subject {
        source_sha: source_sha.to_owned(),
        signer_workflow_path: SIGNER_WORKFLOW.to_owned(),
        signer_run_id: positive(origin.at("run_id"))?,
        signer_run_attempt: receipt.at("signer").at("run_attempt").as_u64().unwrap_or(0),
        receipt_sha256: digest(receipt_bytes),
        linux: (target("linux", "name"), target("linux", "sha256")),
        darwin: (target("darwin", "name"), target("darwin", "sha256")),
    };
    ctx.attestor.verify(
        receipt_bytes,
        bundle,
        &subject,
        ctx.authority,
        ctx.client.deadline(),
    )?;
    Ok(Pair {
        receipt: receipt_bytes.to_vec(),
        bundle: bundle.to_vec(),
        signer_attempt: subject.signer_run_attempt,
        origin: "artifact",
        artifact_id: None,
    })
}

/// Port of `select_pair`.
pub fn select_pair(
    item: &Value,
    pairs: Vec<Pair>,
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Res<Option<Pair>> {
    let assets = item
        .at("assets")
        .as_array()
        .ok_or_else(|| CiError::new("source preview assets"))?;
    require(assets.len() <= 4)?;
    let names: Vec<&str> = assets.iter().map(|a| a.at("name").text()).collect();
    let all = super::model::asset_names();
    require(
        names.iter().collect::<BTreeSet<_>>().len() == names.len()
            && names.iter().all(|n| all.contains(n)),
    )?;
    require(assets.iter().all(|a| a.at("state").is_s("uploaded")))?;
    let mut compatible = Vec::new();
    for pair in pairs {
        let mut expected: BTreeMap<&str, &[u8]> = BTreeMap::from([
            (RECEIPT, pair.receipt.as_slice()),
            (BUNDLE, pair.bundle.as_slice()),
        ]);
        for (key, _, name) in TARGETS {
            expected.insert(name, payloads.get(key).map_or(&[][..], Vec::as_slice));
        }
        let fits = assets.iter().all(|a| {
            expected.get(a.at("name").text()).is_some_and(|data| {
                a.at("size").is_u(data.len() as u64)
                    && a.at("digest").is_s(&format!("sha256:{}", digest(data)))
            })
        });
        if fits {
            compatible.push(pair);
        }
    }
    if let Some(first) = compatible.iter().map(|p| p.signer_attempt).min() {
        let mut matches: Vec<Pair> = compatible
            .into_iter()
            .filter(|p| p.signer_attempt == first)
            .collect();
        require(matches.len() == 1)?;
        if item.at("draft") == &Value::Bool(false) {
            require(
                item.at("immutable") == &Value::Bool(true)
                    && names.iter().copied().collect::<BTreeSet<_>>() == all,
            )?;
        } else {
            require(item.at("draft") == &Value::Bool(true))?;
        }
        return Ok(matches.pop());
    }
    require(item.at("draft") == &Value::Bool(true) && assets.is_empty())?;
    Ok(None)
}

fn signed_attempt(name: &str) -> Option<u64> {
    let digits = name.strip_prefix("source-preview-signed-")?;
    let valid = !digits.is_empty()
        && !digits.starts_with('0')
        && digits.bytes().all(|b| b.is_ascii_digit());
    if valid { digits.parse().ok() } else { None }
}

/// Port of `prepare_pair`: find every receipt and bundle signed so far for the draft,
/// verify each, and choose the one the release must keep. `Sign` when there is none.
pub fn prepare_pair(
    ctx: &Ctx<'_>,
    source: &Value,
    decision: &Value,
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Res<Selection> {
    require(
        keys_of(decision)
            == BTreeSet::from(["source", "candidate", "release_id", "targets", "draft"]),
    )?;
    require(decision.at("source") == source)?;
    let candidate = decision.at("candidate");
    let marker = candidate_marker(source, candidate)?;
    require(decision.at("targets") == &receipt_targets(source, candidate, payloads)?)?;
    let release_id = positive(decision.at("release_id"))?;
    let item = ctx.client.get(&format!("/releases/{release_id}"))?;
    require(item.at("id").is_u(release_id) && item.at("tag_name") == source.at("tag"))?;
    require(item.at("body").is_s(&marker) && item.at("prerelease") == &Value::Bool(true))?;
    let draft = item
        .at("draft")
        .as_bool()
        .ok_or_else(|| CiError::new("source preview draft"))?;
    let mut pairs = Vec::new();
    if draft {
        pairs = artifact_pairs(ctx, source, candidate, release_id, payloads)?;
    } else {
        let (receipt, bundle) = released_pair(ctx.client, &item, source)?;
        let mut pair = verify_pair(
            ctx, source, candidate, release_id, payloads, &receipt, &bundle,
        )?;
        pair.origin = "release";
        pairs.push(pair);
    }
    if let Some(selected) = select_pair(&item, pairs, payloads)? {
        return Ok(Selection::Reuse(selected));
    }
    let run = source.at("source");
    let attempt = run.at("run_attempt").as_u64().unwrap_or(0);
    successful_job(
        ctx.client,
        source.at("source_sha").text(),
        positive(run.at("run_id"))?,
        attempt,
        ROLES[0].1,
        false,
    )?;
    Ok(Selection::Sign)
}

fn artifact_pairs(
    ctx: &Ctx<'_>,
    source: &Value,
    candidate: &Value,
    release_id: u64,
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Res<Vec<Pair>> {
    let run_id = positive(source.at("source").at("run_id"))?;
    let sha = source.at("source_sha");
    let artifacts = inventory(
        ctx.client,
        &format!("/actions/runs/{run_id}/artifacts"),
        "artifacts",
        20,
    )?;
    let signed: Vec<&str> = artifacts
        .iter()
        .map(|a| a.at("name").text())
        .filter(|n| signed_attempt(n).is_some())
        .collect();
    require(signed.iter().collect::<BTreeSet<_>>().len() == signed.len())?;
    let mut pairs = Vec::new();
    for artifact in &artifacts {
        let Some(attempt) = signed_attempt(artifact.at("name").text()) else {
            continue;
        };
        require(attempt <= 51 && attempt <= ctx.run.run_attempt)?;
        let workflow = artifact.at("workflow_run");
        require(workflow.at("id").is_u(run_id) && workflow.at("head_sha") == sha)?;
        if artifact.at("expired") == &Value::Bool(true) {
            continue;
        }
        require(artifact.at("expired") == &Value::Bool(false))?;
        let attest = ROLES[2].1;
        if successful_job(ctx.client, sha.text(), run_id, attempt, attest, true)?.is_none() {
            continue;
        }
        let expected = BTreeMap::from([(RECEIPT, 65_536), (BUNDLE, MAX_JSON)]);
        let files = super::github::artifact_files(ctx.client, artifact, &expected)?;
        let receipt = files
            .get(RECEIPT)
            .ok_or_else(|| CiError::new("source preview receipt"))?;
        let bundle = files
            .get(BUNDLE)
            .ok_or_else(|| CiError::new("source preview bundle"))?;
        let parsed = parse(receipt, usize::MAX)?;
        require(parsed.is_object() && canonical(&parsed) == *receipt)?;
        let same = parsed.at("candidate_source_artifact_id") == candidate.at("source_artifact_id")
            && TARGETS.iter().all(|(key, ..)| {
                parsed
                    .at("targets")
                    .at(key)
                    .at("candidate_payload_artifact_id")
                    == candidate.at(&format!("{key}_payload_artifact_id"))
            });
        if !same {
            continue;
        }
        let mut pair = verify_pair(
            ctx, source, candidate, release_id, payloads, receipt, bundle,
        )?;
        require(pair.signer_attempt == attempt)?;
        pair.artifact_id = Some(positive(artifact.at("id"))?);
        pairs.push(pair);
    }
    Ok(pairs)
}

/// Port of `release`: read the release a receipt names and check its marker and state.
pub fn release(ctx: &Ctx<'_>, receipt: &Value, require_draft: bool) -> Res<Value> {
    let id = positive(receipt.at("release_id"))?;
    let item = ctx.client.get(&format!("/releases/{id}"))?;
    let targets: serde_json::Map<String, Value> = receipt
        .at("targets")
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), json!({"producer": v.at("producer"), "qualification_job_id": v.at("qualification_job_id")})))
        .collect();
    let mut source = receipt.clone();
    source["targets"] = Value::Object(targets);
    let mut candidate = serde_json::Map::new();
    candidate.insert(
        "source_artifact_id".to_owned(),
        receipt.at("candidate_source_artifact_id").clone(),
    );
    for (key, value) in receipt.at("targets").as_object().into_iter().flatten() {
        candidate.insert(
            format!("{key}_payload_artifact_id"),
            value.at("candidate_payload_artifact_id").clone(),
        );
    }
    let marker = candidate_marker(&source, &Value::Object(candidate))?;
    require(item.at("id").is_u(id) && item.at("tag_name") == receipt.at("tag"))?;
    require(item.at("body").is_s(&marker) && item.at("prerelease") == &Value::Bool(true))?;
    require(item.at("draft") == &Value::Bool(require_draft))?;
    Ok(item)
}

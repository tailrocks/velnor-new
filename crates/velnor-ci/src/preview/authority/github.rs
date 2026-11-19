// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::model::{
    BUNDLE, MARKER_PREFIX, RECEIPT, TARGETS, asset_names, candidate_marker, metadata,
    payload_identity, positive,
};
use super::{Client, zip};
use crate::error::CiError;
use crate::preview::json::{Field, Res, canonical, digest, is_hex, parse, require};
use crate::preview::release::{asset_metadata, checked_asset};
use crate::preview::{MAX_JSON, MAX_PAYLOAD, MAX_RECEIPT, REPOSITORY, SIGNER_WORKFLOW};

/// Port of `inventory`: every item of a paged list, in at most `pages` pages of 100,
/// with the count the first page promised and unique ids.
pub fn inventory(client: &dyn Client, path: &str, key: &str, pages: usize) -> Res<Vec<Value>> {
    let mut result: Vec<Value> = Vec::new();
    let mut total: Option<u64> = None;
    for page in 1..=pages {
        let response = client.get(&format!("{path}?per_page=100&page={page}"))?;
        let count = response.at("total_count").as_u64();
        let values = response.at(key).as_array();
        require(count.is_some_and(|c| c <= pages as u64 * 100) && values.is_some())?;
        let (count, values) = (count.unwrap_or(0), values.map_or(&[][..], Vec::as_slice));
        require(total.is_none_or(|t| t == count) && values.len() <= 100)?;
        total = Some(count);
        result.extend(values.iter().cloned());
        require(result.len() as u64 <= count)?;
        if result.len() as u64 == count {
            break;
        }
        require(values.len() == 100)?;
    }
    require(Some(result.len() as u64) == total)?;
    let ids: BTreeSet<u64> = result
        .iter()
        .map(|v| positive(v.at("id")))
        .collect::<Res<_>>()?;
    require(ids.len() == result.len())?;
    Ok(result)
}

/// Port of `successful_job`. Returns the job id, or `None` when `allow_failed` and the
/// job finished without success.
pub fn successful_job(
    client: &dyn Client,
    source: &str,
    run_id: u64,
    attempt: u64,
    name: &str,
    allow_failed: bool,
) -> Res<Option<u64>> {
    require(attempt > 0 && attempt <= 51 && run_id > 0)?;
    let path = format!("/actions/runs/{run_id}/attempts/{attempt}");
    let run = client.get(&path)?;
    require(run.at("id").is_u(run_id) && run.at("run_attempt").is_u(attempt))?;
    require(
        run.at("head_sha").is_s(source)
            && run.at("head_branch").is_s(&format!("sdk-preview-{source}")),
    )?;
    require(run.at("event").is_s("push"))?;
    require(run.at("path").text().split('@').next() == Some(SIGNER_WORKFLOW))?;
    require(run.at("head_repository").at("full_name").is_s(REPOSITORY))?;
    let jobs = inventory(client, &format!("{path}/jobs"), "jobs", 10)?;
    require(
        jobs.iter()
            .all(|j| j.at("run_id").is_u(run_id) && j.at("head_sha").is_s(source)),
    )?;
    let matches: Vec<&Value> = jobs.iter().filter(|j| j.at("name").is_s(name)).collect();
    require(matches.len() == 1 && matches[0].at("status").is_s("completed"))?;
    if allow_failed && !matches[0].at("conclusion").is_s("success") {
        return Ok(None);
    }
    require(matches[0].at("conclusion").is_s("success"))?;
    Ok(matches[0].at("id").as_u64())
}

/// Port of `find_release`: the one release of the tag, if any, in at most 20 pages.
pub fn find_release(client: &dyn Client, tag: &str) -> Res<Option<Value>> {
    let mut releases: Vec<Value> = Vec::new();
    let mut complete = false;
    for page in 1..=20 {
        let values = client.get(&format!("/releases?per_page=100&page={page}"))?;
        let values = values
            .as_array()
            .ok_or_else(|| CiError::new("source preview releases"))?;
        require(values.len() <= 100)?;
        releases.extend(values.iter().cloned());
        if values.len() < 100 {
            complete = true;
            break;
        }
    }
    require(complete)?;
    let ids: BTreeSet<u64> = releases
        .iter()
        .map(|v| positive(v.at("id")))
        .collect::<Res<_>>()?;
    require(ids.len() == releases.len())?;
    let mut matches = releases.into_iter().filter(|r| r.at("tag_name").is_s(tag));
    let first = matches.next();
    require(matches.next().is_none())?;
    Ok(first)
}

/// Port of `verify_tag`: the annotated tag is on the source commit, and `main` is at
/// or ahead of it.
pub fn verify_tag(client: &dyn Client, source: &str) -> Res<()> {
    let tag = format!("sdk-preview-{source}");
    let reference = client.get(&format!("/git/ref/tags/{tag}"))?;
    let object = reference.at("object");
    let oid = object.at("sha").text();
    require(object.at("type").is_s("tag") && is_hex(oid, 40))?;
    let annotated = client.get(&format!("/git/tags/{oid}"))?;
    let target = annotated.at("object");
    require(
        annotated.at("tag").is_s(&tag)
            && target.at("type").is_s("commit")
            && target.at("sha").is_s(source),
    )?;
    let main = client.get("/git/ref/heads/main")?;
    let main = main.at("object");
    require(main.at("type").is_s("commit") && is_hex(main.at("sha").text(), 40))?;
    let comparison = client.get(&format!("/compare/{source}...{}", main.at("sha").text()))?;
    require(matches!(
        comparison.at("status").as_str(),
        Some("ahead" | "identical")
    ))?;
    require(comparison.at("merge_base_commit").at("sha").is_s(source))
}

/// Port of `artifact_files`: download an Actions artifact and unpack the files in
/// `expected`.
pub fn artifact_files(
    client: &dyn Client,
    artifact: &Value,
    expected: &BTreeMap<&str, usize>,
) -> Res<BTreeMap<String, Vec<u8>>> {
    let id = positive(artifact.at("id"))?;
    let size = positive(artifact.at("size_in_bytes"))?;
    let want = artifact.at("digest").text();
    require(size <= 257 * 1024 * 1024)?;
    require(want.strip_prefix("sha256:").is_some_and(|d| is_hex(d, 64)))?;
    let body = client.artifact_zip(id, size, want)?;
    require(body.len() as u64 == size && format!("sha256:{}", digest(&body)) == want)?;
    zip::files(&body, expected)
}

/// Port of `load_candidate`: reload the candidate a draft release was created with,
/// from its marker, and read back the source metadata and payloads of its artifacts.
pub fn load_candidate(
    client: &dyn Client,
    current: &Value,
    marker: &Value,
    run: &super::model::Run<'_>,
) -> Res<(Value, Value, BTreeMap<String, Vec<u8>>)> {
    let keys: BTreeSet<&str> = marker
        .as_object()
        .map_or_else(BTreeSet::new, |m| m.keys().map(String::as_str).collect());
    require(keys == BTreeSet::from(["schema", "source_sha", "run_id", "targets", "candidate"]))?;
    require(marker.at("schema").as_i64() == Some(3))?;
    require(marker.at("source_sha") == current.at("source_sha"))?;
    require(
        marker.at("run_id") == current.at("source").at("run_id")
            && marker.at("targets") == current.at("targets"),
    )?;
    let candidate = marker.at("candidate");
    candidate_marker(current, candidate)?;
    let mut items: BTreeMap<String, Value> = BTreeMap::new();
    for (key, identity) in candidate.as_object().into_iter().flatten() {
        let artifact = client.get(&format!("/actions/artifacts/{}", positive(identity)?))?;
        require(artifact.at("id") == identity && artifact.at("expired") == &Value::Bool(false))?;
        let workflow = artifact.at("workflow_run");
        require(
            workflow.at("id") == marker.at("run_id")
                && workflow.at("head_sha") == current.at("source_sha"),
        )?;
        items.insert(key.clone(), artifact);
    }
    let source_item = items
        .get("source_artifact_id")
        .ok_or_else(|| CiError::new("source preview candidate"))?;
    let files = artifact_files(
        client,
        source_item,
        &BTreeMap::from([("source-preview-metadata.json", 65_536)]),
    )?;
    let raw = files
        .get("source-preview-metadata.json")
        .ok_or_else(|| CiError::new("source preview metadata"))?;
    let original = metadata(raw, run)?;
    let attempt = original
        .at("source")
        .at("run_attempt")
        .as_u64()
        .unwrap_or(0);
    require(
        source_item
            .at("name")
            .is_s(&format!("source-preview-source-{attempt}")),
    )?;
    require(
        original.at("targets") == current.at("targets")
            && original.at("version") == current.at("version"),
    )?;
    successful_job(
        client,
        current.at("source_sha").text(),
        positive(marker.at("run_id"))?,
        attempt,
        "Source preview publication / Admit source preview",
        false,
    )?;
    let mut payloads = BTreeMap::new();
    for (key, _, name) in TARGETS {
        let item = items
            .get(&format!("{key}_payload_artifact_id"))
            .ok_or_else(|| CiError::new("source preview candidate"))?;
        require(
            item.at("name")
                .is_s(&format!("source-preview-qualified-payload-{key}-{attempt}")),
        )?;
        let mut files = artifact_files(client, item, &BTreeMap::from([(name, MAX_PAYLOAD)]))?;
        let data = files
            .remove(name)
            .ok_or_else(|| CiError::new("source preview payload"))?;
        payload_identity(key, &data)?;
        payloads.insert(key.to_owned(), data);
    }
    Ok((original, candidate.clone(), payloads))
}

/// Port of `released_pair`: the receipt and bundle of a published, immutable release.
pub fn released_pair(client: &dyn Client, item: &Value, source: &Value) -> Res<(Vec<u8>, Vec<u8>)> {
    require(item.at("draft") == &Value::Bool(false) && item.at("immutable") == &Value::Bool(true))?;
    let entries = item
        .at("assets")
        .as_array()
        .ok_or_else(|| CiError::new("source preview assets"))?;
    require(entries.len() == 4)?;
    let names: BTreeSet<&str> = entries
        .iter()
        .filter_map(|e| e.at("name").as_str())
        .collect();
    require(entries.len() == names.len() && names == asset_names())?;
    let prefix = format!(
        "https://github.com/{REPOSITORY}/releases/download/{}/",
        source.at("tag").text()
    );
    let mut result = Vec::new();
    for (name, limit) in [(RECEIPT, MAX_RECEIPT), (BUNDLE, MAX_JSON)] {
        let url = format!("{prefix}{name}");
        let (size, sha) = asset_metadata(item, name, limit, &url)?;
        result.push(checked_asset(client, &url, size, &sha, limit)?);
    }
    let bundle = result.pop().unwrap_or_default();
    let receipt = result.pop().unwrap_or_default();
    Ok((receipt, bundle))
}

/// Port of `existing_assets`: the asset names a release already holds, each of the
/// payloads and the receipt with the size and digest of the file it must be.
pub fn existing_assets(
    item: &Value,
    expected: &BTreeMap<String, Vec<u8>>,
    allow_unknown_bundle: bool,
) -> Res<BTreeSet<String>> {
    let assets = item
        .at("assets")
        .as_array()
        .ok_or_else(|| CiError::new("source preview assets"))?;
    require(assets.len() <= 4)?;
    let mut result = BTreeSet::new();
    for asset in assets {
        let name = asset.at("name").text();
        require(
            asset_names().contains(name)
                && !result.contains(name)
                && asset.at("state").is_s("uploaded"),
        )?;
        if let Some(data) = expected.get(name) {
            require(asset.at("size").is_u(data.len() as u64))?;
            require(asset.at("digest").is_s(&format!("sha256:{}", digest(data))))?;
        } else {
            require(name == BUNDLE && allow_unknown_bundle)?;
        }
        result.insert(name.to_owned());
    }
    Ok(result)
}

/// The names and digests an existing draft's assets are held to for the draft decision.
pub fn check_draft_assets(current: &Value, payloads: &BTreeMap<String, Vec<u8>>) -> Res<()> {
    let assets = current
        .at("assets")
        .as_array()
        .ok_or_else(|| CiError::new("source preview assets"))?;
    require(assets.len() <= 4)?;
    let names: Vec<&str> = assets.iter().map(|a| a.at("name").text()).collect();
    let unique: BTreeSet<&str> = names.iter().copied().collect();
    require(unique.len() == names.len() && names.iter().all(|n| asset_names().contains(n)))?;
    for asset in assets {
        require(asset.at("state").is_s("uploaded"))?;
        for (key, _, name) in TARGETS {
            if asset.at("name").is_s(name) {
                let data = payloads
                    .get(key)
                    .ok_or_else(|| CiError::new("source preview payload"))?;
                require(asset.at("size").is_u(data.len() as u64))?;
                require(asset.at("digest").is_s(&format!("sha256:{}", digest(data))))?;
            }
        }
    }
    Ok(())
}

/// Parse the marker a draft release carries in its body.
pub fn parse_marker(body: &Value) -> Res<Value> {
    let text = body
        .as_str()
        .ok_or_else(|| CiError::new("source preview marker"))?;
    let encoded = text
        .strip_prefix(MARKER_PREFIX)
        .ok_or_else(|| CiError::new("source preview marker"))?;
    let marker = parse(encoded.as_bytes(), usize::MAX)?;
    require(canonical(&marker) == encoded.as_bytes())?;
    Ok(marker)
}

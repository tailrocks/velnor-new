// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use super::json::{Field, Res, canonical, digest, parse, require, sha40};
use super::net::{Net, repo_path};
use super::record::receipt_from_record;
use super::{MAX_BUNDLE, MAX_PAYLOAD, MAX_RECEIPT, REPOSITORY};
use crate::error::CiError;

/// The signed receipt, its bundle and the two payloads of a published preview.
#[derive(Debug)]
pub struct Release {
    pub receipt: Vec<u8>,
    pub bundle: Vec<u8>,
    pub linux: Vec<u8>,
    pub darwin: Vec<u8>,
}

/// Port of `checked_asset`: the body must have exactly the size and digest asked for.
pub fn checked_asset(
    net: &dyn Net,
    url: &str,
    size: u64,
    expected: &str,
    maximum: usize,
) -> Res<Vec<u8>> {
    require(size > 0 && size <= maximum as u64 && super::json::is_hex(expected, 64))?;
    let body = net.asset(url, maximum)?;
    require(body.len() as u64 == size && digest(&body) == expected)?;
    Ok(body)
}

/// Port of `asset_metadata`: size and sha256 of the one uploaded asset called `name`.
pub fn asset_metadata(
    release: &Value,
    name: &str,
    maximum: usize,
    url: &str,
) -> Res<(u64, String)> {
    let assets = release
        .at("assets")
        .as_array()
        .ok_or_else(|| CiError::new("assets"))?;
    let matches: Vec<&Value> = assets.iter().filter(|a| a.at("name").is_s(name)).collect();
    require(matches.len() == 1)?;
    let entry = matches[0];
    require(entry.at("state").is_s("uploaded") && entry.at("size").is_pos())?;
    let size = entry.at("size").as_u64().unwrap_or(0);
    require(size <= maximum as u64 && entry.at("browser_download_url").is_s(url))?;
    let digest = entry.at("digest").text();
    require(digest.starts_with("sha256:") && super::json::is_hex(&digest[7..], 64))?;
    Ok((size, digest[7..].to_owned()))
}

/// Port of `verify_annotated_tag`: `sdk-preview-<source>` is an annotated tag on the
/// source commit. Returns the tag object, so a caller can see it did not move.
pub fn verify_annotated_tag(net: &dyn Net, preview: &Value) -> Res<String> {
    let tag = preview.at("tag").text();
    let reference = net.api(&repo_path(&format!("/git/ref/tags/{tag}")))?;
    require(reference.at("ref").is_s(&format!("refs/tags/{tag}")))?;
    let object = reference.at("object");
    require(object.at("type").is_s("tag") && sha40(object.at("sha")))?;
    let sha = object.at("sha").text();
    let annotated = net.api(&repo_path(&format!("/git/tags/{sha}")))?;
    require(annotated.at("sha").is_s(sha) && annotated.at("tag").is_s(tag))?;
    let target = annotated.at("object");
    require(target.at("type").is_s("commit") && target.at("sha") == preview.at("source_sha"))?;
    Ok(sha.to_owned())
}

/// The release holds exactly these four assets, each once.
fn require_exactly(release: &Value, names: &[&str; 4]) -> Res<()> {
    let assets = release
        .at("assets")
        .as_array()
        .ok_or_else(|| CiError::new("assets"))?;
    require(assets.len() == names.len())?;
    let mut have: Vec<&str> = assets
        .iter()
        .filter_map(|a| a.at("name").as_str())
        .collect();
    let mut want = names.to_vec();
    have.sort_unstable();
    want.sort_unstable();
    require(have == want)
}

/// Port of `download_verified_release`.
pub fn download_verified_release(net: &dyn Net, preview: &Value) -> Res<Release> {
    let id = preview.at("release_id");
    let release = net.api(&repo_path(&format!("/releases/{id}")))?;
    require(release.at("id") == id && release.at("tag_name") == preview.at("tag"))?;
    verify_annotated_tag(net, preview)?;
    require(
        release.at("draft") == &Value::Bool(false)
            && release.at("prerelease") == &Value::Bool(true)
            && release.at("immutable") == &Value::Bool(true),
    )?;
    let linux = preview.at("linux_payload_name").text();
    let darwin = preview.at("darwin_payload_name").text();
    let names = [
        "SDK_PREVIEW_RECEIPT.json",
        "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl",
        linux,
        darwin,
    ];
    require_exactly(&release, &names)?;
    let tag = preview.at("tag").text();
    let prefix = format!("https://github.com/{REPOSITORY}/releases/download/{tag}/");
    let (receipt_size, receipt_digest) = asset_metadata(
        &release,
        names[0],
        MAX_RECEIPT,
        &format!("{prefix}{}", names[0]),
    )?;
    let (bundle_size, bundle_digest) = asset_metadata(
        &release,
        names[1],
        MAX_BUNDLE,
        &format!("{prefix}{}", names[1]),
    )?;
    require(
        preview.at("receipt_sha256").is_s(&receipt_digest)
            && preview.at("bundle_sha256").is_s(&bundle_digest),
    )?;
    let receipt = checked_asset(
        net,
        &format!("{prefix}{}", names[0]),
        receipt_size,
        &receipt_digest,
        MAX_RECEIPT,
    )?;
    let bundle = checked_asset(
        net,
        &format!("{prefix}{}", names[1]),
        bundle_size,
        &bundle_digest,
        MAX_BUNDLE,
    )?;
    let mut payloads = Vec::new();
    for (key, name) in [("linux", linux), ("darwin", darwin)] {
        let (size, sha) = asset_metadata(&release, name, MAX_PAYLOAD, &format!("{prefix}{name}"))?;
        require(preview.at(&format!("{key}_payload_size")).is_u(size))?;
        require(preview.at(&format!("{key}_payload_sha256")).is_s(&sha))?;
        payloads.push(checked_asset(
            net,
            &format!("{prefix}{name}"),
            size,
            &sha,
            MAX_PAYLOAD,
        )?);
    }
    let parsed = parse(&receipt, MAX_RECEIPT)?;
    require(parsed == receipt_from_record(preview))?;
    require(canonical(&parsed) == receipt)?;
    let darwin_payload = payloads.pop().unwrap_or_default();
    let linux_payload = payloads.pop().unwrap_or_default();
    Ok(Release {
        receipt,
        bundle,
        linux: linux_payload,
        darwin: darwin_payload,
    })
}

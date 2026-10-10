//! Pure validation of the fixed release and attestation API response shapes.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

use crate::error::HostError;

use super::super::hash::is_sha256_digest;
use super::super::manifest::{ARCHIVE_ASSET, CHECKSUM_ASSET, MANIFEST_ASSET, REPOSITORY};
use super::Asset;

const PRODUCT_REPOSITORY_ID: u64 = 1_390_620_900;
const MAX_ATTESTATIONS: usize = 16;
const MAX_NON_PROBE_ASSET_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    target_commitish: String,
    draft: bool,
    prerelease: bool,
    immutable: bool,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    id: u64,
    name: String,
    state: String,
    size: u64,
    digest: String,
}

pub(super) fn parse_release(
    source: &str,
    value: Value,
) -> Result<BTreeMap<String, Asset>, HostError> {
    let tag = format!("runner-{source}");
    let release: Release = serde_json::from_value(value).map_err(|_| HostError::Identity)?;
    if release.tag_name != tag
        || release.target_commitish != source
        || release.draft
        || release.prerelease
        || !release.immutable
    {
        return Err(HostError::Identity);
    }
    let mut assets = BTreeMap::new();
    for asset in release.assets {
        if asset.id == 0
            || asset.state != "uploaded"
            || asset.size == 0
            || asset.size > MAX_NON_PROBE_ASSET_BYTES
            || !is_sha256_digest(&asset.digest)
            || assets
                .insert(
                    asset.name.clone(),
                    Asset {
                        id: asset.id,
                        name: asset.name,
                        size: asset.size,
                        digest: asset.digest,
                    },
                )
                .is_some()
        {
            return Err(HostError::Identity);
        }
    }
    let expected = BTreeSet::from([
        "velnor-runner-linux-amd64.tar",
        "velnor-dind-linux-amd64.tar",
        ARCHIVE_ASSET,
        MANIFEST_ASSET,
        CHECKSUM_ASSET,
    ]);
    if assets.len() != expected.len() || assets.keys().any(|name| !expected.contains(name.as_str()))
    {
        return Err(HostError::Identity);
    }
    let archive = assets.get(ARCHIVE_ASSET).ok_or(HostError::Identity)?;
    let manifest = assets.get(MANIFEST_ASSET).ok_or(HostError::Identity)?;
    let checksum = assets.get(CHECKSUM_ASSET).ok_or(HostError::Identity)?;
    if archive.size > 16 * 1024 * 1024 || manifest.size > 64 * 1024 || checksum.size > 64 * 1024 {
        return Err(HostError::Frame);
    }
    Ok(assets)
}

pub(super) fn attestation_path(checksum_digest: &str) -> Result<String, HostError> {
    let digest = checksum_digest
        .strip_prefix("sha256:")
        .ok_or(HostError::Identity)?;
    if !crate::artifact_admission::hash::is_lower_hex(digest, 64) {
        return Err(HostError::Identity);
    }
    Ok(format!("repos/{REPOSITORY}/attestations/sha256:{digest}"))
}

pub(super) fn parse_attestations(response: &Value) -> Result<Vec<Value>, HostError> {
    let rows = response
        .get("attestations")
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty() && rows.len() <= MAX_ATTESTATIONS)
        .ok_or(HostError::Identity)?;
    rows.iter().map(parse_attestation).collect()
}

fn parse_attestation(row: &Value) -> Result<Value, HostError> {
    if row.get("repository_id").and_then(Value::as_u64) != Some(PRODUCT_REPOSITORY_ID)
        || row
            .get("repository")
            .and_then(Value::as_str)
            .is_some_and(|repo| repo != REPOSITORY)
    {
        return Err(HostError::Identity);
    }
    let bundle = row
        .get("bundle")
        .filter(|bundle| bundle.is_object())
        .ok_or(HostError::Identity)?;
    if bundle.get("mediaType").and_then(Value::as_str)
        != Some("application/vnd.dev.sigstore.bundle.v0.3+json")
        || !bundle.get("dsseEnvelope").is_some_and(Value::is_object)
        || !bundle
            .get("verificationMaterial")
            .is_some_and(Value::is_object)
    {
        return Err(HostError::Identity);
    }
    Ok(bundle.clone())
}

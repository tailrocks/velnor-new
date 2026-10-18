//! Release asset inventory, checksums, and downloaded-file validation.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::super::{ProviderFailure, hex};
use super::{
    ASSETS, CHECKSUM_ASSETS, CHECKSUMS_FILE, MANIFEST_FILE, MAX_ARCHIVE_BYTES, MAX_CHECKSUMS_BYTES,
    MAX_MANIFEST_BYTES, PROBE_ARCHIVE, RELEASE_SIGNER, SOURCE_REPOSITORY,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AssetMetadata {
    pub(super) size: u64,
    pub(super) digest: String,
}

pub(super) fn release_metadata(
    value: &Value,
    tag: &str,
    source_revision: &str,
) -> Result<HashMapAssets, ProviderFailure> {
    let object = value.as_object().ok_or(ProviderFailure::ReleaseMetadata)?;
    if string(object.get("tagName")) != Some(tag)
        || string(object.get("targetCommitish")) != Some(source_revision)
        || boolean(object.get("isDraft")) != Some(false)
        || boolean(object.get("isImmutable")) != Some(true)
        || boolean(object.get("isPrerelease")) != Some(false)
    {
        return Err(ProviderFailure::ReleaseTarget);
    }
    let array = object
        .get("assets")
        .and_then(Value::as_array)
        .ok_or(ProviderFailure::ReleaseAssets)?;
    if array.len() != ASSETS.len() {
        return Err(ProviderFailure::ReleaseAssets);
    }
    let mut assets = HashMapAssets::new();
    for item in array {
        let item = item.as_object().ok_or(ProviderFailure::ReleaseAssets)?;
        let name = string(item.get("name")).ok_or(ProviderFailure::ReleaseAssets)?;
        let digest = string(item.get("digest")).ok_or(ProviderFailure::ReleaseAssets)?;
        let size = item
            .get("size")
            .and_then(Value::as_u64)
            .ok_or(ProviderFailure::ReleaseAssets)?;
        let state = string(item.get("state")).ok_or(ProviderFailure::ReleaseAssets)?;
        if !ASSETS.contains(&name)
            || assets
                .insert(
                    name.to_owned(),
                    AssetMetadata {
                        size,
                        digest: digest.to_owned(),
                    },
                )
                .is_some()
            || state != "uploaded"
            || size == 0
            || !is_sha256_digest(digest)
        {
            return Err(ProviderFailure::ReleaseAssets);
        }
    }
    if ASSETS.iter().any(|name| !assets.contains_key(*name)) {
        return Err(ProviderFailure::ReleaseAssets);
    }
    if assets[PROBE_ARCHIVE].size > MAX_ARCHIVE_BYTES
        || assets[MANIFEST_FILE].size > MAX_MANIFEST_BYTES
        || assets[CHECKSUMS_FILE].size > MAX_CHECKSUMS_BYTES
    {
        return Err(ProviderFailure::ReleaseAssets);
    }
    Ok(assets)
}

pub(super) type HashMapAssets = HashMap<String, AssetMetadata>;

pub(super) fn verify_release_signature(
    value: &Value,
    tag: &str,
    source_revision: &str,
    assets: &HashMapAssets,
) -> Result<(), ProviderFailure> {
    let verification = value
        .get("verificationResult")
        .ok_or(ProviderFailure::ReleaseSignature)?;
    let signer = verification
        .pointer("/signature/certificate/subjectAlternativeName")
        .and_then(Value::as_str);
    let statement = verification
        .get("statement")
        .ok_or(ProviderFailure::ReleaseSignature)?;
    if signer != Some(RELEASE_SIGNER)
        || statement.pointer("/_type").and_then(Value::as_str)
            != Some("https://in-toto.io/Statement/v1")
        || statement.pointer("/predicateType").and_then(Value::as_str)
            != Some("https://in-toto.io/attestation/release/v0.2")
        || statement
            .pointer("/predicate/repository")
            .and_then(Value::as_str)
            != Some(SOURCE_REPOSITORY)
        || statement.pointer("/predicate/tag").and_then(Value::as_str) != Some(tag)
    {
        return Err(ProviderFailure::ReleaseSignature);
    }
    let subjects = statement
        .get("subject")
        .and_then(Value::as_array)
        .ok_or(ProviderFailure::ReleaseSignature)?;
    if subjects.len() != ASSETS.len() + 1 {
        return Err(ProviderFailure::ReleaseSignature);
    }
    let mut seen = std::collections::HashSet::new();
    let release_subject = format!("pkg:github/{SOURCE_REPOSITORY}@{tag}");
    for subject in subjects {
        if subject.get("uri").and_then(Value::as_str) == Some(release_subject.as_str()) {
            let expected = subject.pointer("/digest/sha1").and_then(Value::as_str);
            if expected != Some(source_revision) {
                return Err(ProviderFailure::ReleaseSignature);
            }
            seen.insert("release".to_owned());
            continue;
        }
        let name = string(subject.get("name")).ok_or(ProviderFailure::ReleaseSignature)?;
        let digest = subject
            .pointer("/digest/sha256")
            .and_then(Value::as_str)
            .ok_or(ProviderFailure::ReleaseSignature)?;
        let Some(metadata) = assets.get(name) else {
            return Err(ProviderFailure::ReleaseSignature);
        };
        if metadata.digest != format!("sha256:{digest}") || !seen.insert(name.to_owned()) {
            return Err(ProviderFailure::ReleaseSignature);
        }
    }
    if seen.len() != ASSETS.len() + 1 || !seen.contains("release") {
        return Err(ProviderFailure::ReleaseSignature);
    }
    Ok(())
}

pub(super) fn parse_checksums(bytes: &[u8]) -> Result<HashMapAssets, ProviderFailure> {
    let text = std::str::from_utf8(bytes).map_err(|_| ProviderFailure::AssetChecksum)?;
    if !text.ends_with('\n') {
        return Err(ProviderFailure::AssetChecksum);
    }
    let mut checksums = HashMapAssets::new();
    for line in text[..text.len() - 1].split('\n') {
        let (digest, name) = line
            .split_once("  ")
            .ok_or(ProviderFailure::AssetChecksum)?;
        if !valid_sha256(digest)
            || !CHECKSUM_ASSETS.contains(&name)
            || checksums
                .insert(
                    name.to_owned(),
                    AssetMetadata {
                        size: 0,
                        digest: format!("sha256:{digest}"),
                    },
                )
                .is_some()
        {
            return Err(ProviderFailure::AssetChecksum);
        }
    }
    if checksums.len() != CHECKSUM_ASSETS.len()
        || CHECKSUM_ASSETS
            .iter()
            .any(|name| !checksums.contains_key(*name))
    {
        return Err(ProviderFailure::AssetChecksum);
    }
    Ok(checksums)
}

pub(super) fn asset_hashes(
    archive: &[u8],
    manifest: &[u8],
    checksums: &[u8],
) -> std::collections::HashMap<&'static str, String> {
    [
        (PROBE_ARCHIVE, sha256(archive)),
        (MANIFEST_FILE, sha256(manifest)),
        (CHECKSUMS_FILE, sha256(checksums)),
    ]
    .into_iter()
    .collect()
}

pub(super) fn verify_asset_digests(
    metadata: &HashMapAssets,
    checksums: &HashMapAssets,
    hashes: &std::collections::HashMap<&'static str, String>,
) -> Result<(), ProviderFailure> {
    for name in CHECKSUM_ASSETS {
        let expected = metadata
            .get(name)
            .ok_or(ProviderFailure::AssetChecksum)?
            .digest
            .strip_prefix("sha256:")
            .ok_or(ProviderFailure::AssetChecksum)?;
        let checksum = checksums.get(name).ok_or(ProviderFailure::AssetChecksum)?;
        if checksum.digest != format!("sha256:{expected}")
            || hashes
                .get(name)
                .is_some_and(|actual| actual.as_str() != expected)
        {
            return Err(ProviderFailure::AssetChecksum);
        }
    }
    let sums_digest = metadata
        .get(CHECKSUMS_FILE)
        .ok_or(ProviderFailure::AssetChecksum)?
        .digest
        .strip_prefix("sha256:")
        .ok_or(ProviderFailure::AssetChecksum)?;
    if hashes.get(CHECKSUMS_FILE).map(String::as_str) != Some(sums_digest) {
        return Err(ProviderFailure::AssetChecksum);
    }
    Ok(())
}

pub(super) fn ensure_exact_downloads(
    assets: &Path,
    expected: &[&str],
) -> Result<(), ProviderFailure> {
    let mut names = fs::read_dir(assets)
        .map_err(|_| ProviderFailure::ReleaseAssets)?
        .map(|entry| {
            entry
                .map_err(|_| ProviderFailure::ReleaseAssets)?
                .file_name()
                .into_string()
                .map_err(|_| ProviderFailure::ReleaseAssets)
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    if names.iter().map(String::as_str).ne(expected) {
        return Err(ProviderFailure::ReleaseAssets);
    }
    Ok(())
}

pub(super) fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, ProviderFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ProviderFailure::ReleaseAssets)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > max_bytes {
        return Err(ProviderFailure::ReleaseAssets);
    }
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|_| ProviderFailure::ReleaseAssets)?;
    let capacity = usize::try_from(metadata.len().min(max_bytes))
        .map_err(|_| ProviderFailure::ReleaseAssets)?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| ProviderFailure::ReleaseAssets)?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > max_bytes {
        return Err(ProviderFailure::ReleaseAssets);
    }
    Ok(bytes)
}

pub(super) fn string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

fn boolean(value: Option<&Value>) -> Option<bool> {
    value.and_then(Value::as_bool)
}

pub(super) fn number(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}

pub(super) fn valid_digest_field(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(is_sha256_digest)
}

pub(super) fn valid_hex_field(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(valid_sha256)
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(valid_sha256)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

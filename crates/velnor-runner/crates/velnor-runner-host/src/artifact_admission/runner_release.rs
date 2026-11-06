// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use reqwest::Client;

use crate::compile_identity::CompiledReleaseIdentity;
use crate::error::HostError;
use crate::helper_supervisor::ChecksumTarget;
use crate::resource_probe_release::runner_manifest::{self, model::RunnerImageManifest};

use super::github::{self, Asset};
use super::hash::{is_lower_hex, lowercase_hex, sha256};

const MANIFEST: &str = "RUNNER_IMAGE_MANIFEST.json";
const CHECKSUMS: &str = "SHA256SUMS";
const MAX_BYTES: usize = 64 * 1024;

pub(super) struct VerifiedRunnerRelease {
    pub(super) source_sha: String,
    pub(super) manifest_sha: String,
    pub(super) manifest: RunnerImageManifest,
}

pub(super) async fn fetch_verified(
    client: &Client,
    pat: &str,
    identity: &CompiledReleaseIdentity,
    state_directory: &Path,
) -> Result<VerifiedRunnerRelease, HostError> {
    let source_sha = lowercase_hex(identity.source_sha());
    let assets = github::runner_release(client, pat, &source_sha).await?;
    let checksum_asset = asset(&assets, CHECKSUMS)?;
    let manifest_asset = asset(&assets, MANIFEST)?;
    let checksum_bytes = github::download_asset(client, pat, checksum_asset, MAX_BYTES).await?;
    validate_bytes(checksum_asset, &checksum_bytes)?;
    let checksums = parse_checksums(&checksum_bytes)?;
    for name in [MANIFEST, "LICENSE", "NOTICE"] {
        if checksums.get(name).map(String::as_str) != Some(asset_hex(asset(&assets, name)?)?) {
            return Err(HostError::Identity);
        }
    }
    let manifest_bytes = github::download_asset(client, pat, manifest_asset, MAX_BYTES).await?;
    validate_bytes(manifest_asset, &manifest_bytes)?;
    let checksum_digest = asset_hex(checksum_asset)?;
    let manifest_digest = asset_hex(manifest_asset)?;
    let bundles = github::attestations(client, pat, &checksum_asset.digest).await?;
    let mut valid = 0_usize;
    for bundle in bundles {
        if crate::helper_supervisor::verify_checksum_target(
            identity.helper_sha256(),
            ChecksumTarget {
                state_directory,
                bundle: &bundle,
                checksum_bytes: &checksum_bytes,
                source_sha: &source_sha,
                authority_sha: &source_sha,
                checksum_digest,
                target_name: MANIFEST,
                target_digest: manifest_digest,
            },
        )
        .await
        .is_ok()
        {
            valid = valid.checked_add(1).ok_or(HostError::Identity)?;
        }
    }
    if valid != 1 {
        return Err(HostError::Identity);
    }
    let manifest = runner_manifest::parse_for_source(
        &manifest_bytes,
        &source_sha,
        crate::compile_identity::runner_lock(),
    )?;
    persist_manifest(state_directory, &source_sha, &manifest_bytes)?;
    Ok(VerifiedRunnerRelease {
        source_sha,
        manifest_sha: manifest_digest.to_owned(),
        manifest,
    })
}

fn parse_checksums(bytes: &[u8]) -> Result<BTreeMap<String, String>, HostError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES || !bytes.ends_with(b"\n") {
        return Err(HostError::Frame);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| HostError::Identity)?;
    let expected = BTreeSet::from([MANIFEST, "LICENSE", "NOTICE"]);
    let mut result = BTreeMap::new();
    for line in text
        .strip_suffix('\n')
        .ok_or(HostError::Identity)?
        .split('\n')
    {
        let (digest, name) = line.split_once("  ").ok_or(HostError::Identity)?;
        if !is_lower_hex(digest, 64)
            || !expected.contains(name)
            || result.insert(name.to_owned(), digest.to_owned()).is_some()
        {
            return Err(HostError::Identity);
        }
    }
    if result.len() != expected.len() {
        return Err(HostError::Identity);
    }
    Ok(result)
}

fn asset<'a>(assets: &'a BTreeMap<String, Asset>, name: &str) -> Result<&'a Asset, HostError> {
    assets.get(name).ok_or(HostError::Identity)
}

fn asset_hex(asset: &Asset) -> Result<&str, HostError> {
    asset
        .digest
        .strip_prefix("sha256:")
        .filter(|value| is_lower_hex(value, 64))
        .ok_or(HostError::Identity)
}

fn validate_bytes(asset: &Asset, bytes: &[u8]) -> Result<(), HostError> {
    if u64::try_from(bytes.len()).map_err(|_| HostError::Frame)? != asset.size
        || asset_hex(asset)? != lowercase_hex(&sha256(bytes))
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

#[cfg(unix)]
fn persist_manifest(state_directory: &Path, source: &str, bytes: &[u8]) -> Result<(), HostError> {
    use rustix::fs::{AtFlags, Mode, OFlags, linkat, open, openat, unlinkat};
    use std::io::{Read, Write};
    use std::os::unix::fs::MetadataExt;

    if !state_directory.is_absolute() || !is_lower_hex(source, 40) {
        return Err(HostError::Path);
    }
    let directory = std::fs::File::from(
        open(
            state_directory,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| HostError::Path)?,
    );
    let metadata = directory.metadata().map_err(|_| HostError::Path)?;
    if !metadata.is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o022 != 0
    {
        return Err(HostError::Path);
    }
    let final_name = format!("runner-image-manifest-{source}.json");
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| HostError::Identity)?;
    let temporary_name = format!(".runner-image-manifest-{}", lowercase_hex(&random));
    let mut file = std::fs::File::from(
        openat(
            &directory,
            &temporary_name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|_| HostError::Path)?,
    );
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        unlinkat(&directory, &temporary_name, AtFlags::empty()).map_err(|_| HostError::Path)?;
        return Err(HostError::Path);
    }
    let linked = linkat(
        &directory,
        &temporary_name,
        &directory,
        &final_name,
        AtFlags::empty(),
    );
    let removed = unlinkat(&directory, &temporary_name, AtFlags::empty());
    if removed.is_err() {
        return Err(HostError::Path);
    }
    if linked.is_err() {
        let existing = openat(
            &directory,
            &final_name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| HostError::Path)?;
        let existing = std::fs::File::from(existing);
        let existing_meta = existing.metadata().map_err(|_| HostError::Path)?;
        if !existing_meta.is_file()
            || existing_meta.uid() != metadata.uid()
            || existing_meta.mode() & 0o022 != 0
        {
            return Err(HostError::Path);
        }
        let mut existing_bytes = Vec::new();
        existing
            .take(u64::try_from(MAX_BYTES + 1).map_err(|_| HostError::Frame)?)
            .read_to_end(&mut existing_bytes)
            .map_err(|_| HostError::Path)?;
        if existing_bytes != bytes {
            return Err(HostError::Identity);
        }
    }
    directory.sync_all().map_err(|_| HostError::Path)
}

#[cfg(not(unix))]
fn persist_manifest(
    _state_directory: &Path,
    _source: &str,
    _bytes: &[u8],
) -> Result<(), HostError> {
    Err(HostError::Path)
}

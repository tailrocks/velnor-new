// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CheckDeadline, GitHubApi, OrchestratorError, PublicReleaseAsset, ReleaseAssetResponse,
    ReleaseResponse, asset_tuples, asset_tuples_from_downloads, github, internal,
};

#[cfg(any(target_os = "linux", test))]
pub(crate) fn read_historical_sdk_attestation_assets(
    version: &str,
    source_sha: &str,
    manifest_sha256: &str,
    deadline: CheckDeadline,
) -> Result<(Vec<u8>, Vec<u8>), OrchestratorError> {
    let invalid = || internal("historical_sdk_assets_invalid");
    if !velnor_actions_contract::config::is_exact_semver(version)
        || source_sha.len() != 40
        || !source_sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || manifest_sha256.len() != 64
        || !manifest_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let tag = format!("v{version}");
    let api = GitHubApi::new("tailrocks/velnor-new".to_owned()).with_deadline(deadline);
    api.verify_preview_repository_identity(1_390_620_900, "main")?;
    let release = api.release_by_tag(&tag)?.ok_or_else(invalid)?;
    let identity = api.tag_identity(&tag)?.ok_or_else(invalid)?;
    if release.id == 0
        || release.draft != Some(false)
        || release.prerelease != Some(false)
        || release.immutable != Some(true)
        || identity.source_sha != source_sha
    {
        return Err(invalid());
    }
    let assets = api.complete_release_assets(release.id)?;
    let read = |name: &str, limit: usize| -> Result<Vec<u8>, OrchestratorError> {
        let asset = assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or_else(invalid)?;
        if asset.size == 0 || asset.size > u64::try_from(limit).map_err(|_| invalid())? {
            return Err(invalid());
        }
        let bytes = api.download_asset_bounded(asset.id, limit)?;
        if bytes.len() as u64 != asset.size
            || asset
                .digest
                .as_ref()
                .is_some_and(|d| d != &format!("sha256:{}", github::sha256(&bytes)))
        {
            return Err(invalid());
        }
        Ok(bytes)
    };
    let manifest = read("release-manifest.json", 1024 * 1024)?;
    let bundle = read("release-manifest.json.intoto.jsonl", 4 * 1024 * 1024)?;
    if github::sha256(&manifest) != manifest_sha256 {
        return Err(invalid());
    }
    let after = api.release_by_tag(&tag)?.ok_or_else(invalid)?;
    let after_tag = api.tag_identity(&tag)?.ok_or_else(invalid)?;
    if after.id != release.id
        || after.draft != release.draft
        || after.prerelease != release.prerelease
        || after.immutable != release.immutable
        || after_tag.source_sha != identity.source_sha
        || after_tag.message != identity.message
        || super::asset_tuples(&api.complete_release_assets(release.id)?)
            != super::asset_tuples(&assets)
    {
        return Err(invalid());
    }
    Ok((manifest, bundle))
}

pub(super) fn download_release_assets(
    api: &GitHubApi,
    assets: Vec<ReleaseAssetResponse>,
    deadline: CheckDeadline,
) -> Result<Vec<PublicReleaseAsset>, OrchestratorError> {
    let mut downloaded = Vec::with_capacity(assets.len());
    for asset in assets {
        deadline
            .remaining()
            .map_err(|_| internal("package_public_verification_deadline"))?;
        let bytes = api.download_asset(asset.id)?;
        if bytes.len() as u64 != asset.size {
            return Err(internal("package_public_asset_size_mismatch"));
        }
        let sha256 = github::sha256(&bytes);
        if asset
            .digest
            .as_deref()
            .is_some_and(|digest| digest != format!("sha256:{sha256}"))
        {
            return Err(internal("package_public_asset_digest_mismatch"));
        }
        downloaded.push(PublicReleaseAsset {
            id: asset.id,
            name: asset.name,
            size: asset.size,
            sha256,
            api_digest: asset.digest,
            bytes,
        });
    }
    Ok(downloaded)
}

pub(super) fn verify_public_release_unchanged(
    api: &GitHubApi,
    tag: &str,
    release: &ReleaseResponse,
    tag_identity: &github::TagObservation,
    downloaded: &[PublicReleaseAsset],
) -> Result<(), OrchestratorError> {
    let final_release = api
        .release_by_tag(tag)?
        .ok_or_else(|| internal("package_public_release_disappeared"))?;
    let final_assets = api.complete_release_assets(release.id)?;
    if final_release.id != release.id
        || final_release.tag_name != release.tag_name
        || final_release.draft != release.draft
        || final_release.prerelease != release.prerelease
        || final_release.immutable != release.immutable
        || final_release.body != release.body
        || asset_tuples(&final_assets) != asset_tuples_from_downloads(downloaded)
    {
        return Err(internal("package_public_release_changed"));
    }
    let final_tag = api
        .tag_identity(tag)?
        .ok_or_else(|| internal("package_public_tag_disappeared"))?;
    if final_tag.source_sha != tag_identity.source_sha || final_tag.message != tag_identity.message
    {
        return Err(internal("package_public_tag_changed"));
    }
    Ok(())
}

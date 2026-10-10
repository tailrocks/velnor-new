//! Validate exact release bytes and their shared signed checksum provenance.

use reqwest::Client;

use crate::compile_identity::CompiledReleaseIdentity;
use crate::error::HostError;

use super::archive::{self, ArchiveIdentity};
use super::checksums::{self, MAX_CHECKSUM_BYTES};
use super::github::{self, Asset};
use super::hash::{is_lower_hex, lowercase_hex, sha256};
use super::manifest::{self, ARCHIVE_ASSET, CHECKSUM_ASSET, MANIFEST_ASSET, ProbeManifest};
use crate::helper_supervisor::ChecksumTarget;

pub(super) struct VerifiedRelease {
    pub(super) source_sha: String,
    pub(super) authority_sha: String,
    pub(super) archive_sha: String,
    pub(super) archive: Vec<u8>,
    pub(super) manifest: ProbeManifest,
    pub(super) archive_identity: ArchiveIdentity,
}

pub(super) async fn fetch_verified(
    client: &Client,
    pat: &str,
    identity: &CompiledReleaseIdentity,
) -> Result<VerifiedRelease, HostError> {
    if !github::valid_pat(pat) {
        return Err(HostError::Identity);
    }
    let source_sha = lowercase_hex(identity.source_sha());
    let authority_sha = source_sha.clone();
    if !is_lower_hex(&source_sha, 40) {
        return Err(HostError::Identity);
    }
    let assets = github::release(client, pat, &source_sha).await?;
    let checksum_asset = asset(&assets, CHECKSUM_ASSET)?;
    let manifest_asset = asset(&assets, MANIFEST_ASSET)?;
    let archive_asset = asset(&assets, ARCHIVE_ASSET)?;
    let checksum_bytes =
        github::download_asset(client, pat, checksum_asset, MAX_CHECKSUM_BYTES).await?;
    validate_asset_bytes(checksum_asset, &checksum_bytes)?;
    let checksums = checksums::parse(&checksum_bytes)?;
    validate_release_checksum_rows(&assets, &checksums)?;
    let manifest_bytes =
        github::download_asset(client, pat, manifest_asset, manifest::MAX_MANIFEST_BYTES).await?;
    validate_asset_bytes(manifest_asset, &manifest_bytes)?;
    let probe_manifest = manifest::parse(&manifest_bytes, identity)?;
    if probe_manifest.workflow_authority_sha != authority_sha {
        return Err(HostError::Identity);
    }
    let archive_bytes =
        github::download_asset(client, pat, archive_asset, 16 * 1024 * 1024).await?;
    validate_asset_bytes(archive_asset, &archive_bytes)?;
    let archive_sha = lowercase_hex(&sha256(&archive_bytes));
    if probe_manifest.archive_sha256 != archive_sha
        || checksums::asset_digest(&checksums, ARCHIVE_ASSET)? != archive_sha
        || checksums::asset_digest(&checksums, MANIFEST_ASSET)?
            != lowercase_hex(&sha256(&manifest_bytes))
    {
        return Err(HostError::Identity);
    }
    verify_provenance(
        client,
        pat,
        ProvenanceClaims {
            identity,
            source_sha: &source_sha,
            authority_sha: &authority_sha,
            checksum_bytes: &checksum_bytes,
            checksum_asset,
            manifest_asset,
            archive_asset,
        },
    )
    .await?;
    let archive_identity = archive::validate(&archive_bytes, &probe_manifest)?;
    Ok(VerifiedRelease {
        source_sha,
        authority_sha,
        archive_sha,
        archive: archive_bytes,
        manifest: probe_manifest,
        archive_identity,
    })
}

struct ProvenanceClaims<'a> {
    identity: &'a CompiledReleaseIdentity,
    source_sha: &'a str,
    authority_sha: &'a str,
    checksum_bytes: &'a [u8],
    checksum_asset: &'a Asset,
    manifest_asset: &'a Asset,
    archive_asset: &'a Asset,
}

async fn verify_provenance(
    client: &Client,
    pat: &str,
    claims: ProvenanceClaims<'_>,
) -> Result<(), HostError> {
    let bundles = github::attestations(client, pat, &claims.checksum_asset.digest).await?;
    let checksum_digest = claims
        .checksum_asset
        .digest
        .strip_prefix("sha256:")
        .ok_or(HostError::Identity)?;
    let mut valid = 0_usize;
    for bundle in bundles {
        let manifest_valid = crate::helper_supervisor::verify_checksum_target(
            claims.identity.helper_sha256(),
            ChecksumTarget {
                bundle: &bundle,
                checksum_bytes: claims.checksum_bytes,
                source_sha: claims.source_sha,
                authority_sha: claims.authority_sha,
                checksum_digest,
                target_name: MANIFEST_ASSET,
                target_digest: asset_hex(claims.manifest_asset)?,
            },
        )
        .await
        .is_ok();
        let archive_valid = crate::helper_supervisor::verify_checksum_target(
            claims.identity.helper_sha256(),
            ChecksumTarget {
                bundle: &bundle,
                checksum_bytes: claims.checksum_bytes,
                source_sha: claims.source_sha,
                authority_sha: claims.authority_sha,
                checksum_digest,
                target_name: ARCHIVE_ASSET,
                target_digest: asset_hex(claims.archive_asset)?,
            },
        )
        .await
        .is_ok();
        if manifest_valid && archive_valid {
            valid = valid.checked_add(1).ok_or(HostError::Identity)?;
        }
    }
    if valid == 1 {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

fn validate_release_checksum_rows(
    assets: &std::collections::BTreeMap<String, Asset>,
    checksums: &std::collections::BTreeMap<String, String>,
) -> Result<(), HostError> {
    for name in [
        "velnor-runner-linux-amd64.tar",
        "velnor-dind-linux-amd64.tar",
        ARCHIVE_ASSET,
        MANIFEST_ASSET,
    ] {
        let asset = assets.get(name).ok_or(HostError::Identity)?;
        if asset_hex(asset)? != checksums::asset_digest(checksums, name)? {
            return Err(HostError::Identity);
        }
    }
    Ok(())
}

fn asset<'a>(
    assets: &'a std::collections::BTreeMap<String, Asset>,
    name: &str,
) -> Result<&'a Asset, HostError> {
    assets.get(name).ok_or(HostError::Identity)
}

fn asset_hex(asset: &Asset) -> Result<&str, HostError> {
    asset
        .digest
        .strip_prefix("sha256:")
        .filter(|value| is_lower_hex(value, 64))
        .ok_or(HostError::Identity)
}

fn validate_asset_bytes(asset: &Asset, bytes: &[u8]) -> Result<(), HostError> {
    if u64::try_from(bytes.len()).map_err(|_| HostError::Frame)? != asset.size
        || asset_hex(asset)? != lowercase_hex(&sha256(bytes))
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_asset_bytes;
    use crate::artifact_admission::github::Asset;
    use crate::artifact_admission::hash::lowercase_hex;
    use crate::error::HostError;
    use sha2::{Digest, Sha256};

    #[test]
    fn checks_download_size_and_digest() -> Result<(), HostError> {
        let bytes = b"manifest";
        let asset = Asset {
            id: 1,
            name: "RESOURCE_PROBE_MANIFEST.json".to_owned(),
            size: u64::try_from(bytes.len()).map_err(|_| HostError::Frame)?,
            digest: format!("sha256:{}", lowercase_hex(&Sha256::digest(bytes))),
        };
        validate_asset_bytes(&asset, bytes)?;
        assert_eq!(
            validate_asset_bytes(&asset, b"tampered"),
            Err(HostError::Identity)
        );
        Ok(())
    }
}

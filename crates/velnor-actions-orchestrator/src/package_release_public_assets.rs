// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, path::Path};

use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::{PackageReleaseChannel, PackageReleaseReceipt},
};

use super::OrchestratorError;
use crate::{
    internal::internal, package_release_publish::PublicReleaseSnapshot,
    package_release_signer::PackageReleaseSigningIdentity,
};

pub(super) fn verified_file_inventory(
    snapshot: &PublicReleaseSnapshot,
    receipt: &PackageReleaseReceipt,
) -> Result<BTreeMap<String, Vec<u8>>, OrchestratorError> {
    let expected = receipt
        .assets
        .iter()
        .map(|asset| (asset.name.as_str(), asset.size, asset.sha256.as_str()))
        .chain([("release.json", 0, ""), ("release.json.bundle", 0, "")])
        .collect::<Vec<_>>();
    if snapshot.assets.len() != expected.len() {
        return Err(internal("package_public_asset_inventory_mismatch"));
    }
    let mut files = BTreeMap::new();
    for asset in &snapshot.assets {
        if !safe_asset_name(&asset.name) || files.contains_key(&asset.name) {
            return Err(internal("package_public_asset_name_invalid"));
        }
        if u64::try_from(asset.bytes.len()).ok() != Some(asset.size)
            || sha256(&asset.bytes) != asset.sha256
        {
            return Err(internal("package_public_asset_digest_mismatch"));
        }
        if asset.name == "release.json" {
            if asset.bytes.is_empty() {
                return Err(internal("package_public_receipt_digest_mismatch"));
            }
        } else if asset.name == "release.json.bundle" {
            if asset.bytes.is_empty() {
                return Err(internal("package_public_bundle_invalid"));
            }
        } else {
            let declared = receipt
                .assets
                .iter()
                .find(|declared| declared.name == asset.name)
                .ok_or_else(|| internal("package_public_asset_inventory_mismatch"))?;
            if declared.size != asset.size
                || declared.sha256 != asset.sha256
                || u64::try_from(asset.bytes.len()).ok() != Some(asset.size)
            {
                return Err(internal("package_public_asset_digest_mismatch"));
            }
        }
        files.insert(asset.name.clone(), asset.bytes.clone());
    }
    for (name, size, digest) in expected {
        let bytes = files
            .get(name)
            .ok_or_else(|| internal("package_public_asset_inventory_mismatch"))?;
        if size > 0 && u64::try_from(bytes.len()).ok() != Some(size) {
            return Err(internal("package_public_asset_size_mismatch"));
        }
        if !digest.is_empty() && sha256(bytes) != digest {
            return Err(internal("package_public_asset_digest_mismatch"));
        }
    }
    Ok(files)
}

pub(super) fn signing_identity(
    receipt: &PackageReleaseReceipt,
    plan: &PackageReleasePlan,
    source_sha: &str,
) -> Result<PackageReleaseSigningIdentity, OrchestratorError> {
    let identity = PackageReleaseSigningIdentity {
        repository: receipt.repository.slug.clone(),
        repository_id: receipt.repository.id,
        source_sha: source_sha.to_owned(),
        source_ref: receipt.source.source_ref.clone(),
        default_branch: receipt.source.default_branch.clone(),
        event: receipt.source.event,
        workflow_path: receipt.workflow.path.clone(),
        workflow_ref: signing_workflow_ref(
            &plan.repository,
            &receipt.workflow.path,
            &receipt.source.source_ref,
        ),
        workflow_sha: receipt.workflow.authority_sha.clone(),
        run_id: receipt.workflow.run_id,
        run_attempt: receipt.workflow.run_attempt,
        ci_run_id: receipt.source.ci_run_id,
        ci_run_attempt: receipt.source.ci_run_attempt,
        commit_count: receipt.version.commit_count,
    };
    if !is_full_sha(&identity.workflow_sha)
        || identity.repository != plan.repository
        || identity.workflow_path != plan.channel.workflow_path()
    {
        return Err(internal("package_public_signer_identity_mismatch"));
    }
    Ok(identity)
}

pub(super) fn signing_workflow_ref(
    repository: &str,
    workflow_path: &str,
    source_ref: &str,
) -> String {
    format!(
        "{}/{}@{}",
        repository,
        workflow_path.trim_start_matches('/'),
        source_ref
    )
}

pub(super) fn public_asset_size_limits(
    plan: &PackageReleasePlan,
) -> Result<BTreeMap<String, u64>, OrchestratorError> {
    const RECEIPT_MAX_BYTES: u64 = 1024 * 1024;
    const RECEIPT_BUNDLE_MAX_BYTES: u64 = 8 * 1024 * 1024;
    let mut limits = BTreeMap::from([
        ("release.json".to_owned(), RECEIPT_MAX_BYTES),
        ("release.json.bundle".to_owned(), RECEIPT_BUNDLE_MAX_BYTES),
    ]);
    for product in &plan.products {
        for target in &product.targets {
            let payload = format!("{}-{target}.tar.gz", product.id);
            if limits.insert(payload.clone(), 1024 * 1024 * 1024).is_some()
                || limits
                    .insert(format!("{payload}.sbom.json"), 32 * 1024 * 1024)
                    .is_some()
                || limits
                    .insert(format!("{payload}.sha256"), 1024 * 1024)
                    .is_some()
                || limits
                    .insert(format!("{payload}.bundle"), RECEIPT_BUNDLE_MAX_BYTES)
                    .is_some()
            {
                return Err(internal("package_public_plan_asset_collision"));
            }
        }
    }
    if limits
        .insert("SHA256SUMS".to_owned(), 1024 * 1024)
        .is_some()
    {
        return Err(internal("package_public_plan_asset_collision"));
    }
    for companion in &plan.companions {
        if limits
            .insert(companion.asset_name.clone(), 1024 * 1024 * 1024)
            .is_some()
            || limits
                .insert(
                    format!("{}.bundle", companion.asset_name),
                    RECEIPT_BUNDLE_MAX_BYTES,
                )
                .is_some()
        {
            return Err(internal("package_public_plan_asset_collision"));
        }
    }
    Ok(limits)
}

pub(super) fn tag_for(
    plan: &PackageReleasePlan,
    source_sha: &str,
) -> Result<String, OrchestratorError> {
    if !is_full_sha(source_sha) {
        return Err(internal("package_public_source_sha_invalid"));
    }
    match plan.channel {
        PackageReleaseChannel::Preview => Ok(format!("preview-{source_sha}")),
        PackageReleaseChannel::Stable => Ok(format!("v{}", plan.base_version)),
    }
}

pub(super) fn safe_asset_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name
            .bytes()
            .any(|byte| byte == b'/' || byte == b'\\' || byte.is_ascii_control())
}

pub(super) fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut output = String::with_capacity(64);
    for byte in sha2::Sha256::digest(bytes) {
        output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        output.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| internal("package_public_asset_create_failed"))?;
    file.write_all(bytes)
        .map_err(|_| internal("package_public_asset_write_failed"))?;
    file.sync_all()
        .map_err(|_| internal("package_public_asset_sync_failed"))
}

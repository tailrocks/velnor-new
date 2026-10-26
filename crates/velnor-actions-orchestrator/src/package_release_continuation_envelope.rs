// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{
    github::{GITHUB_ACTIONS_BOT_ID, ReleaseResponse, sha256},
    identity::{MAX_RECOVERY_BODY_BYTES, make_recovery_body},
};
use crate::{OrchestratorError, internal::internal};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::{
        PackageReleaseChannel, PackageReleaseReceipt, parse_package_release_receipt,
    },
    preview_identity::PreviewIdentity,
};

pub(super) fn preview_receipt_hint(
    release: &ReleaseResponse,
    plan: &PackageReleasePlan,
    repo_id: u64,
    source_sha: &str,
    commit_count: u64,
    tag_message: &str,
) -> Result<PackageReleaseReceipt, OrchestratorError> {
    let identity = PreviewIdentity::derive(&plan.base_version, source_sha, commit_count)
        .map_err(|_| invalid_preview())?;
    if release.id == 0
        || release.tag_name != identity.tag
        || release.draft != Some(false)
        || release.prerelease != Some(true)
        || release.immutable != Some(true)
        || !release.author.as_ref().is_some_and(|author| {
            author.id == Some(GITHUB_ACTIONS_BOT_ID)
                && author.login.as_deref() == Some("github-actions[bot]")
        })
        || release.body.as_deref() != Some(tag_message)
        || tag_message.len() > MAX_RECOVERY_BODY_BYTES
    {
        return Err(invalid_preview());
    }
    let (receipt_bytes, bundle, digest) =
        parse_recovery_body(tag_message).map_err(|_| invalid_preview())?;
    if bundle.is_empty()
        || sha256(&receipt_bytes) != digest
        || make_recovery_body(&receipt_bytes, &bundle, digest).map_err(|_| invalid_preview())?
            != tag_message
    {
        return Err(invalid_preview());
    }
    let receipt = parse_package_release_receipt(&receipt_bytes).map_err(|_| invalid_preview())?;
    receipt
        .validate_against_plan(plan)
        .map_err(|_| invalid_preview())?;
    if receipt.channel != PackageReleaseChannel::Preview
        || receipt.repository.slug != plan.repository
        || receipt.repository.id != repo_id
        || receipt.source.default_branch != plan.default_branch
        || receipt.source.commit_sha != source_sha
        || receipt.tag != identity.tag
        || receipt.version.base != plan.base_version
        || receipt.version.value != identity.version
        || receipt.version.commit_count != Some(commit_count)
        || receipt.version.source_sha7.as_deref() != Some(&source_sha[..7])
    {
        return Err(invalid_preview());
    }
    Ok(receipt)
}

pub(super) fn stable_receipt_hint(
    release: &ReleaseResponse,
    plan: &PackageReleasePlan,
    repo_id: u64,
    tag_message: &str,
) -> Result<PackageReleaseReceipt, OrchestratorError> {
    if release.id == 0
        || release.tag_name != format!("v{}", plan.base_version)
        || release.draft != Some(false)
        || release.prerelease != Some(false)
        || release.immutable != Some(true)
        || !release.author.as_ref().is_some_and(|author| {
            author.id == Some(GITHUB_ACTIONS_BOT_ID)
                && author.login.as_deref() == Some("github-actions[bot]")
        })
        || release.body.as_deref() != Some(tag_message)
        || tag_message.len() > MAX_RECOVERY_BODY_BYTES
    {
        return Err(invalid());
    }
    let (receipt, bundle, digest) = parse_recovery_body(tag_message)?;
    if bundle.is_empty()
        || sha256(&receipt) != digest
        || make_recovery_body(&receipt, &bundle, digest)? != tag_message
    {
        return Err(invalid());
    }
    let receipt = parse_package_release_receipt(&receipt).map_err(|_| invalid())?;
    receipt.validate_against_plan(plan).map_err(|_| invalid())?;
    if receipt.channel != PackageReleaseChannel::Stable
        || receipt.repository.slug != plan.repository
        || receipt.repository.id != repo_id
        || receipt.source.default_branch != plan.default_branch
        || receipt.tag != format!("v{}", plan.base_version)
        || receipt.version.base != plan.base_version
        || receipt.version.value != plan.base_version
        || receipt.version.commit_count.is_some()
    {
        return Err(invalid());
    }
    Ok(receipt)
}

fn parse_recovery_body(body: &str) -> Result<(Vec<u8>, Vec<u8>, &str), OrchestratorError> {
    let lines = body.split_terminator('\n').collect::<Vec<_>>();
    if lines.len() != 4 || lines[0] != "velnor-package-release-recovery-v1" {
        return Err(invalid());
    }
    let digest = lines[1]
        .strip_prefix("receipt_sha256=")
        .ok_or_else(invalid)?;
    let receipt = URL_SAFE_NO_PAD
        .decode(lines[2].strip_prefix("receipt=").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let bundle = URL_SAFE_NO_PAD
        .decode(lines[3].strip_prefix("bundle=").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    Ok((receipt, bundle, digest))
}

fn invalid() -> OrchestratorError {
    internal("tap_continuation_stable_envelope")
}

fn invalid_preview() -> OrchestratorError {
    internal("tap_continuation_preview_envelope")
}

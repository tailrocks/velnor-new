//! Independent channel publication checks before any downstream distribution.

use super::{
    github::{GitHubApi, ReleaseResponse},
    identity::{local_identity, recovery_body},
    run_identity::RunSource,
    stable_tag_origin,
};
use crate::{
    OrchestratorError, internal::internal, package_release_signer::VerifiedAttestedSignedPackage,
};
use std::{collections::BTreeSet, env};
use velnor_actions_contract::package_release::PackageReleaseChannel;

pub(crate) fn verify_published_release(
    signed: &VerifiedAttestedSignedPackage,
) -> Result<(), OrchestratorError> {
    match signed.receipt().channel {
        PackageReleaseChannel::Preview => verify_immutable_preview_release(signed),
        PackageReleaseChannel::Stable => verify_immutable_stable_release(signed),
    }
}

pub(crate) fn verify_immutable_preview_release(
    signed: &VerifiedAttestedSignedPackage,
) -> Result<(), OrchestratorError> {
    if signed.receipt().channel != PackageReleaseChannel::Preview {
        return Err(internal("preview_publication_wrong_channel"));
    }
    let api = authenticated_api(signed)?;
    let receipt = signed.receipt();
    let expected = local_identity(signed)?;
    let expected_body = recovery_body(signed, &expected.receipt_digest)?;
    let mut release = selected_release(&api, &receipt.tag)?;
    if release.id == 0
        || release.draft != Some(false)
        || release.prerelease != Some(true)
        || release.immutable != Some(true)
        || release.body.as_deref() != Some(expected_body.as_str())
    {
        return Err(internal("preview_published_release_state"));
    }
    verify_preview_tag(&api, signed, &expected_body)?;
    release.assets = api.complete_release_assets(release.id)?;
    if release.assets.len() != expected.assets.len()
        || api.remote_assets(&release)? != expected.assets
    {
        return Err(internal("preview_published_asset_inventory"));
    }
    let final_release = selected_release(&api, &receipt.tag)?;
    if final_release.id != release.id
        || final_release.body != release.body
        || final_release.draft != Some(false)
        || final_release.prerelease != Some(true)
        || final_release.immutable != Some(true)
    {
        return Err(internal("preview_published_generation_changed"));
    }
    let final_assets = api.complete_release_assets(release.id)?;
    if asset_tuples(&final_assets) != asset_tuples(&release.assets) {
        return Err(internal("preview_published_inventory_changed"));
    }
    verify_preview_tag(&api, signed, &expected_body)
}

pub(crate) fn verify_immutable_stable_release(
    signed: &VerifiedAttestedSignedPackage,
) -> Result<(), OrchestratorError> {
    if signed.receipt().channel != PackageReleaseChannel::Stable {
        return Err(internal("stable_publication_wrong_channel"));
    }
    let api = authenticated_api(signed)?;
    stable_tag_origin::verify_signed_origin(signed)?;
    let mut release = selected_release(&api, &signed.receipt().tag)?;
    let expected = local_identity(signed)?;
    if release.id == 0
        || release.draft != Some(false)
        || release.prerelease != Some(false)
        || release.immutable != Some(true)
        || release.body.as_deref()
            != Some(recovery_body(signed, &expected.receipt_digest)?.as_str())
    {
        return Err(internal("stable_published_release_state"));
    }
    verify_tag(&api, signed)?;
    release.assets = api.complete_release_assets(release.id)?;
    if api.remote_assets(&release)? != expected.assets {
        return Err(internal("stable_published_asset_bytes"));
    }
    stable_tag_origin::verify_signed_origin(signed)
}

fn authenticated_api(
    signed: &VerifiedAttestedSignedPackage,
) -> Result<GitHubApi, OrchestratorError> {
    let receipt = signed.receipt();
    let api = GitHubApi::new(receipt.repository.slug.clone());
    api.verify_repository(receipt.repository.id)?;
    api.verify_release_channel_environment(receipt.channel, &receipt.source.default_branch)?;
    let retained = env::var("GITHUB_RUN_ID").ok().as_deref()
        != Some(receipt.workflow.run_id.to_string().as_str())
        || env::var("GITHUB_RUN_ATTEMPT").ok().as_deref()
            != Some(receipt.workflow.run_attempt.to_string().as_str());
    api.verify_release_run(receipt, retained)?;
    if retained {
        let invocation = super::recovery::promotion_context::Invocation::from_env()?;
        if invocation.continuation(signed.plan()) {
            invocation.authenticate_continuation(signed.plan(), &api, receipt.repository.id)?;
        } else {
            invocation.authenticate_recovery(signed.plan(), &api, receipt.repository.id)?;
        }
    } else {
        api.verify_direct_release_caller(signed.plan(), receipt)?;
    }

    api.verify_ci_identity(
        receipt.source.ci_run_id,
        receipt.source.ci_run_attempt,
        RunSource {
            repository_id: receipt.repository.id,
            sha: &receipt.source.commit_sha,
            branch: &receipt.source.default_branch,
        },
        false,
    )?;
    Ok(api)
}

fn verify_tag(
    api: &GitHubApi,
    signed: &VerifiedAttestedSignedPackage,
) -> Result<(), OrchestratorError> {
    let tag = api
        .tag_identity(&signed.receipt().tag)?
        .ok_or_else(|| internal("published_release_tag_missing"))?;
    if tag.source_sha != signed.receipt().source.commit_sha {
        return Err(internal("published_release_tag_source"));
    }
    Ok(())
}

fn verify_preview_tag(
    api: &GitHubApi,
    signed: &VerifiedAttestedSignedPackage,
    expected_message: &str,
) -> Result<(), OrchestratorError> {
    let tag = api
        .tag_identity(&signed.receipt().tag)?
        .ok_or_else(|| internal("published_release_tag_missing"))?;
    if tag.source_sha != signed.receipt().source.commit_sha
        || tag.message.as_deref() != Some(expected_message)
    {
        return Err(internal("preview_published_tag_identity"));
    }
    Ok(())
}

fn selected_release(api: &GitHubApi, tag: &str) -> Result<ReleaseResponse, OrchestratorError> {
    api.release_by_tag(tag)?
        .ok_or_else(|| internal("published_release_missing"))
}

fn asset_tuples(
    assets: &[super::github::ReleaseAssetResponse],
) -> BTreeSet<(u64, &str, u64, Option<&str>)> {
    assets
        .iter()
        .map(|asset| {
            (
                asset.id,
                asset.name.as_str(),
                asset.size,
                asset.digest.as_deref(),
            )
        })
        .collect()
}

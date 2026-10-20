//! Independent channel publication checks before any downstream distribution.

use super::{
    current_ci_from_env,
    github::{GitHubApi, ReleaseResponse, unique_release},
    identity::{local_identity, recovery_body},
    recovery::require_successful_job,
    rolling_state::Envelope,
    run_identity::RunSource,
    stable_tag_origin, verify_current_source,
};
use crate::{
    OrchestratorError, internal::internal, package_release_signer::VerifiedAttestedSignedPackage,
};
use std::{collections::BTreeSet, env};
use velnor_actions_contract::{
    package_release::PackageReleaseChannel,
    publication_retry::rolling::{RollingAssetState, RollingOriginal, RollingPhase, staged_name},
};

pub(crate) fn verify_published_release(
    signed: &VerifiedAttestedSignedPackage,
) -> Result<(), OrchestratorError> {
    match signed.receipt().channel {
        PackageReleaseChannel::Preview => verify_committed_preview_release(signed),
        PackageReleaseChannel::Stable => verify_immutable_stable_release(signed),
    }
}

pub(crate) fn verify_committed_preview_release(
    signed: &VerifiedAttestedSignedPackage,
) -> Result<(), OrchestratorError> {
    if signed.receipt().channel != PackageReleaseChannel::Preview {
        return Err(internal("rolling_publication_wrong_channel"));
    }
    let api = authenticated_api(signed)?;
    let receipt = signed.receipt();
    let expected = local_identity(signed)?;
    let mut release = selected_release(&api, "preview")?;
    let release_id = configured_preview_release_id()?;
    if release.id != release_id
        || release.draft != Some(false)
        || release.prerelease != Some(true)
        || release.immutable != Some(false)
    {
        return Err(internal("rolling_published_release_state"));
    }
    let envelope = Envelope::parse(release.body.as_deref())?
        .ok_or_else(|| internal("rolling_published_journal_missing"))?;
    if envelope.schema != "velnor.rolling-preview-journal.v1"
        || envelope.repository != receipt.repository.slug
        || envelope.repository_id != receipt.repository.id
        || envelope.release_id != release_id
        || envelope.tag != "preview"
        || envelope.journal.phase != RollingPhase::Committed
        || envelope.predecessor.is_some()
        || !envelope.journal.obsolete.is_empty()
        || envelope.journal.identity != expected
        || envelope.journal.original != original_pointer(&api, signed)?
        || envelope.journal.staged.keys().ne(expected.assets.keys())
    {
        return Err(internal("rolling_published_journal_identity"));
    }
    verify_latest_preview(&api, signed)?;
    verify_tag(&api, signed)?;
    release.assets = api.complete_release_assets(release_id)?;
    let ids: BTreeSet<_> = release.assets.iter().map(|asset| asset.id).collect();
    if ids.len() != release.assets.len() || release.assets.len() != expected.assets.len() {
        return Err(internal("rolling_published_asset_closure"));
    }
    for (name, captured) in &envelope.journal.staged {
        let actual = release
            .assets
            .iter()
            .find(|asset| asset.id == captured.id)
            .ok_or_else(|| internal("rolling_published_asset_id_changed"))?;
        if captured.name != staged_name(&expected, name)
            || captured.state != RollingAssetState::Uploaded
            || captured.size == 0
            || actual.name != *name
            || actual.size != captured.size
            || captured.sha256.as_ref() != expected.assets.get(name)
        {
            return Err(internal("rolling_published_asset_identity"));
        }
    }
    if api.remote_assets(&release)? != expected.assets {
        return Err(internal("rolling_published_asset_bytes"));
    }
    let final_release = selected_release(&api, "preview")?;
    if final_release.id != release_id
        || final_release.body != release.body
        || final_release.draft != Some(false)
        || final_release.prerelease != Some(true)
        || final_release.immutable != Some(false)
    {
        return Err(internal("rolling_published_generation_changed"));
    }
    let final_assets = api.complete_release_assets(release_id)?;
    if asset_tuples(&final_assets) != asset_tuples(&release.assets) {
        return Err(internal("rolling_published_inventory_changed"));
    }
    verify_tag(&api, signed)?;
    verify_latest_preview(&api, signed)
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

fn verify_latest_preview(
    api: &GitHubApi,
    signed: &VerifiedAttestedSignedPackage,
) -> Result<(), OrchestratorError> {
    let receipt = signed.receipt();
    verify_current_source(
        &receipt.repository.slug,
        &receipt.source.default_branch,
        &receipt.source.commit_sha,
    )?;
    let (id, attempt) = current_ci_from_env()?;
    api.verify_ci_identity(
        id,
        attempt,
        RunSource {
            repository_id: receipt.repository.id,
            sha: &receipt.source.commit_sha,
            branch: &receipt.source.default_branch,
        },
        true,
    )
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

fn selected_release(api: &GitHubApi, tag: &str) -> Result<ReleaseResponse, OrchestratorError> {
    let mut releases = api.release_list(tag)?;
    unique_release(&releases, tag)?;
    releases
        .pop()
        .ok_or_else(|| internal("published_release_missing"))
}

pub(super) fn configured_preview_release_id() -> Result<u64, OrchestratorError> {
    let value = env::var("VELNOR_PREVIEW_RELEASE_ID")
        .map_err(|_| internal("rolling_release_id_missing"))?;
    value
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0 && id.to_string() == value)
        .ok_or_else(|| internal("rolling_release_id_invalid"))
}

pub(super) fn original_pointer(
    api: &GitHubApi,
    signed: &VerifiedAttestedSignedPackage,
) -> Result<RollingOriginal, OrchestratorError> {
    let receipt = signed.receipt();
    let name = format!(
        "velnor-preview-signed-{}-{}",
        receipt.workflow.run_id, receipt.workflow.run_attempt
    );
    let artifacts = api.workflow_artifacts(receipt.workflow.run_id)?;
    let matches: Vec<_> = artifacts
        .iter()
        .filter(|artifact| artifact.name == name)
        .collect();
    if matches.len() != 1 {
        return Err(internal("rolling_original_artifact_ambiguous"));
    }
    let artifact = matches[0];
    let digest = artifact
        .digest
        .as_deref()
        .and_then(|value| value.strip_prefix("sha256:"))
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        })
        .ok_or_else(|| internal("rolling_original_artifact_digest"))?;
    if artifact.id == 0
        || artifact.expired
        || artifact.size_in_bytes == 0
        || artifact.size_in_bytes > 1024 * 1024 * 1024
        || artifact.workflow_run.as_ref().is_none_or(|run| {
            run.id != receipt.workflow.run_id
                || run.repository_id != receipt.repository.id
                || run.head_sha != receipt.source.commit_sha
                || run.head_branch != receipt.source.default_branch
        })
    {
        return Err(internal("rolling_original_artifact_identity"));
    }
    require_successful_job(
        &api.complete_workflow_jobs(receipt.workflow.run_id, receipt.workflow.run_attempt)?,
        receipt.workflow.run_id,
        &receipt.source.commit_sha,
        receipt.channel.signer_job_id(),
    )?;
    Ok(RollingOriginal {
        run_id: receipt.workflow.run_id,
        run_attempt: receipt.workflow.run_attempt,
        artifact_id: artifact.id,
        archive_sha256: digest.to_owned(),
    })
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

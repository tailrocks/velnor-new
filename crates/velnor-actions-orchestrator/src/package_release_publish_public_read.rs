// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::vcs::VcsInputs;

use super::github::ReleaseResponse;
use super::{GitHubApi, ReleaseAssetResponse, asset_tuples, asset_tuples_from_downloads, github};

#[path = "package_release_publish_pull_request_merge_ref.rs"]
mod pull_request_merge_ref;

pub(crate) struct PublicReleaseSnapshot {
    pub(crate) release_id: u64,
    pub(crate) tag: String,
    pub(crate) draft: bool,
    pub(crate) prerelease: bool,
    pub(crate) immutable: bool,
    pub(crate) body: String,
    pub(crate) tag_source_sha: String,
    pub(crate) tag_message: Option<String>,
    pub(crate) assets: Vec<PublicReleaseAsset>,
}

pub(crate) struct PublicReleaseAsset {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
    pub(crate) api_digest: Option<String>,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) struct PublicPullRequestSnapshot {
    pub(crate) state: String,
    pub(crate) draft: bool,
    pub(crate) merged: bool,
    pub(crate) base_repository: String,
    pub(crate) base_repository_id: u64,
    pub(crate) base_ref: String,
    pub(crate) base_sha: String,
    pub(crate) head_repository: String,
    pub(crate) head_repository_id: u64,
    pub(crate) head_ref: String,
    pub(crate) head_sha: String,
    pub(crate) merge_ref_sha: Option<String>,
}

pub(crate) fn read_public_repository_identity(
    repository: &str,
    deadline: CheckDeadline,
) -> Result<u64, OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository) {
        return Err(internal("package_public_repository_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(repository.to_owned()).with_deadline(deadline);
    let observed: github::RepositoryResponse = api.get_json(&format!("repos/{repository}"))?;
    if observed.id == 0 || observed.full_name.as_deref() != Some(repository) {
        return Err(internal("package_public_repository_identity_mismatch"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(observed.id)
}

pub(crate) fn read_public_pull_request(
    repository: &str,
    repository_id: u64,
    number: u64,
    deadline: CheckDeadline,
) -> Result<PublicPullRequestSnapshot, OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository)
        || repository_id == 0
        || number == 0
    {
        return Err(internal("package_public_pull_request_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(repository.to_owned()).with_deadline(deadline);
    api.verify_repository_identity(repository_id)?;
    let response: PublicPullRequestResponse =
        api.get_json(&api.route(&format!("pulls/{number}")))?;
    let base = response.base;
    let head = response.head;
    let (Some(base_repository), Some(head_repository)) = (base.repo, head.repo) else {
        return Err(internal("package_public_pull_request_repository_missing"));
    };
    if response.number != number
        || base_repository.id != repository_id
        || head_repository.id == 0
        || base_repository.full_name != repository
        || !velnor_actions_contract::vcs::is_safe_github_repository(&head_repository.full_name)
        || !github::is_lowercase_git_sha(&base.sha)
        || !github::is_lowercase_git_sha(&head.sha)
        || base.reference.is_empty()
        || head.reference.is_empty()
        || base.reference.bytes().any(|byte| byte.is_ascii_control())
        || head.reference.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(internal("package_public_pull_request_identity_mismatch"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let merged = response.merged_at.is_some();
    let merge_ref_sha = if response.state == "open" && !merged {
        pull_request_merge_ref::read_public_pull_request_merge_ref(
            &api, number, &base.sha, &head.sha, deadline,
        )?
    } else {
        None
    };
    let snapshot = PublicPullRequestSnapshot {
        state: response.state,
        draft: response.draft,
        merged,
        base_repository: base_repository.full_name,
        base_repository_id: base_repository.id,
        base_ref: base.reference,
        base_sha: base.sha,
        head_repository: head_repository.full_name,
        head_repository_id: head_repository.id,
        head_ref: head.reference,
        head_sha: head.sha,
        merge_ref_sha,
    };
    if !valid_head_reference(&snapshot.head_ref) {
        return Err(internal("package_public_pull_request_identity_mismatch"));
    }
    Ok(snapshot)
}

fn valid_head_reference(reference: &str) -> bool {
    VcsInputs {
        commit: None,
        reference: Some(format!("refs/heads/{reference}")),
        submodules: BTreeMap::new(),
    }
    .validate()
    .is_ok()
}

pub(crate) fn read_public_branch_tip(
    repository: &str,
    repository_id: u64,
    branch: &str,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository)
        || repository_id == 0
        || branch.is_empty()
        || branch.len() > 255
        || branch.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(internal("package_public_branch_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(repository.to_owned()).with_deadline(deadline);
    api.verify_repository_identity(repository_id)?;
    let refs: Vec<github::GitRefResponse> = api.get_json(&api.route(&format!(
        "git/matching-refs/heads/{}",
        github::encode_segment(branch)
    )))?;
    let expected = format!("refs/heads/{branch}");
    let exact = refs
        .into_iter()
        .filter(|reference| reference.ref_name == expected)
        .collect::<Vec<_>>();
    if exact.len() != 1
        || exact[0].object.object_type != "commit"
        || !github::is_lowercase_git_sha(&exact[0].object.sha)
    {
        return Err(internal("package_public_branch_identity_mismatch"));
    }
    let sha = exact[0].object.sha.clone();
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(sha)
}

#[derive(serde::Deserialize)]
struct PublicPullRequestResponse {
    number: u64,
    state: String,
    draft: bool,
    merged_at: Option<String>,
    base: PublicPullRequestRef,
    head: PublicPullRequestRef,
}

#[derive(serde::Deserialize)]
struct PublicPullRequestRef {
    #[serde(rename = "ref")]
    reference: String,
    sha: String,
    repo: Option<PublicPullRequestRepository>,
}

#[derive(serde::Deserialize)]
struct PublicPullRequestRepository {
    id: u64,
    full_name: String,
}

pub(crate) fn read_public_release_snapshot(
    repository: &str,
    repository_id: u64,
    default_branch: &str,
    tag: &str,
    asset_size_limits: &BTreeMap<String, u64>,
    deadline: CheckDeadline,
) -> Result<PublicReleaseSnapshot, OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository)
        || repository_id == 0
        || tag.is_empty()
        || tag.len() > 255
        || tag
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'/')
    {
        return Err(internal("package_public_release_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(repository.to_owned()).with_deadline(deadline);
    api.verify_preview_repository_identity(repository_id, default_branch)?;
    let release = api
        .release_by_tag(tag)?
        .ok_or_else(|| internal("package_public_release_missing"))?;
    if release.id == 0 || release.tag_name != tag {
        return Err(internal("package_public_release_identity_mismatch"));
    }
    let tag_identity = api
        .tag_identity(tag)?
        .ok_or_else(|| internal("package_public_tag_missing"))?;
    let assets = api.complete_release_assets(release.id)?;
    let asset_metadata = assets
        .iter()
        .map(|asset| (asset.name.clone(), asset.size))
        .collect::<Vec<_>>();
    let _total_bytes = validate_public_asset_size_inventory(&asset_metadata, asset_size_limits)?;
    let downloaded = download_release_assets(&api, assets, deadline)?;
    verify_public_release_unchanged(&api, tag, &release, &tag_identity, &downloaded)?;
    public_release_snapshot(release, tag_identity, downloaded)
}

fn download_release_assets(
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

fn verify_public_release_unchanged(
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

fn public_release_snapshot(
    release: ReleaseResponse,
    tag_identity: github::TagObservation,
    assets: Vec<PublicReleaseAsset>,
) -> Result<PublicReleaseSnapshot, OrchestratorError> {
    Ok(PublicReleaseSnapshot {
        release_id: release.id,
        tag: release.tag_name,
        draft: release
            .draft
            .ok_or_else(|| internal("package_public_release_state_missing"))?,
        prerelease: release
            .prerelease
            .ok_or_else(|| internal("package_public_release_state_missing"))?,
        immutable: release
            .immutable
            .ok_or_else(|| internal("package_public_release_state_missing"))?,
        body: release
            .body
            .ok_or_else(|| internal("package_public_release_body_missing"))?,
        tag_source_sha: tag_identity.source_sha,
        tag_message: tag_identity.message,
        assets,
    })
}

pub(super) fn validate_public_asset_size_inventory(
    assets: &[(String, u64)],
    limits: &BTreeMap<String, u64>,
) -> Result<u64, OrchestratorError> {
    if assets.is_empty() || assets.len() > 2048 || assets.len() != limits.len() {
        return Err(internal("package_public_asset_inventory_bound"));
    }
    let mut names = std::collections::BTreeSet::new();
    let mut total = 0_u64;
    for (name, size) in assets {
        let maximum = limits
            .get(name)
            .copied()
            .filter(|maximum| *maximum > 0)
            .ok_or_else(|| internal("package_public_asset_inventory_mismatch"))?;
        if !names.insert(name.as_str()) || *size == 0 || *size > maximum {
            return Err(internal("package_public_asset_size_bound"));
        }
        total = total
            .checked_add(*size)
            .filter(|size| *size <= 6 * 1024 * 1024 * 1024)
            .ok_or_else(|| internal("package_public_total_asset_size_bound"))?;
    }
    Ok(total)
}

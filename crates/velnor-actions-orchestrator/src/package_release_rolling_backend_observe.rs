//! Fresh fixed-release observations and monotonic tag transport.

use super::super::{
    github::{GitHubApi, ReleaseResponse, unique_release},
    published,
};
use super::{Envelope, GitHubPackagePublisher, authority, internal};
use crate::OrchestratorError;
use serde::Deserialize;
use velnor_actions_contract::publication_retry::rolling::{
    RollingBootstrapSnapshot, RollingJournal, RollingPhase, RollingReleaseStatus, RollingState,
};

pub(super) fn release(
    publisher: &GitHubPackagePublisher,
    id: u64,
) -> Result<ReleaseResponse, OrchestratorError> {
    let releases = publisher.release_list()?;
    let release = unique_release(&releases, "preview")?;
    if release.id != id
        || release.draft != Some(false)
        || release.prerelease != Some(true)
        || release.immutable != Some(false)
    {
        return Err(internal("rolling_fixed_release_lifecycle"));
    }
    publisher
        .api
        .get_json(&publisher.api.route(&format!("releases/{id}")))
        .and_then(|fresh: ReleaseResponse| {
            if fresh.id != id
                || fresh.tag_name != "preview"
                || fresh.body != release.body
                || fresh.draft != Some(false)
                || fresh.prerelease != Some(true)
                || fresh.immutable != Some(false)
            {
                return Err(internal("rolling_fixed_release_changed"));
            }
            Ok(fresh)
        })
}

pub(super) fn state(
    publisher: &GitHubPackagePublisher,
    id: u64,
    cache: &mut authority::EvidenceCache,
) -> Result<RollingState, OrchestratorError> {
    publisher.verify_remote_repository()?;
    let release = release(publisher, id)?;
    let tag = publisher
        .api
        .tag_identity("preview")?
        .ok_or_else(|| internal("rolling_existing_tag_missing"))?;
    let assets = publisher.api.rolling_assets(id)?;
    let envelope = Envelope::parse(release.body.as_deref())?;
    let replaceable = if let Some(envelope) = &envelope {
        authority::history(publisher, id, envelope, &assets, cache)?
    } else {
        let snapshot = RollingBootstrapSnapshot {
            schema: "velnor.rolling-preview-bootstrap.v1".to_owned(),
            release_id: id,
            tag: "preview".to_owned(),
            tag_source: tag.source_sha.clone(),
            assets: assets.clone(),
        };
        authority::bootstrap(&snapshot)?;
        assets.clone()
    };
    let journal_can_advance = match &envelope {
        Some(previous) => ancestor(
            &publisher.api,
            &previous.journal.identity.source_sha,
            &publisher.identity.source_sha,
        )?,
        None => false,
    };
    let observed = RollingState {
        original: published::original_pointer(&publisher.api, &publisher.signed)?,
        release_id: id,
        tag: "preview".to_owned(),
        status: RollingReleaseStatus::MutablePreview,
        tag_can_fast_forward: ancestor(
            &publisher.api,
            &tag.source_sha,
            &publisher.identity.source_sha,
        )?,
        tag_source: tag.source_sha,
        assets,
        replaceable,
        published: envelope
            .as_ref()
            .filter(|item| item.journal.phase == RollingPhase::Committed)
            .map(|item| item.journal.identity.clone()),
        journal: envelope.map(|item| item.journal),
        journal_can_advance,
    };
    let final_release = self::release(publisher, id)?;
    if final_release.body != release.body {
        return Err(internal("rolling_observation_body_changed"));
    }
    Ok(observed)
}

pub(super) fn bootstrap(state: &RollingState) -> RollingBootstrapSnapshot {
    RollingBootstrapSnapshot {
        schema: "velnor.rolling-preview-bootstrap.v1".to_owned(),
        release_id: state.release_id,
        tag: "preview".to_owned(),
        tag_source: state.tag_source.clone(),
        assets: state.assets.clone(),
    }
}

pub(super) fn envelope(
    publisher: &GitHubPackagePublisher,
    id: u64,
    journal: RollingJournal,
) -> Envelope {
    Envelope {
        schema: "velnor.rolling-preview-journal.v1".to_owned(),
        repository: publisher.receipt.repository.slug.clone(),
        repository_id: publisher.receipt.repository.id,
        release_id: id,
        tag: "preview".to_owned(),
        journal,
        predecessor: None,
    }
}

#[derive(Deserialize)]
struct Comparison {
    status: String,
    ahead_by: u64,
    behind_by: u64,
    merge_base_commit: Commit,
    base_commit: Commit,
}
#[derive(Deserialize)]
struct Commit {
    sha: String,
}

pub(super) fn ancestor(
    api: &GitHubApi,
    before: &str,
    after: &str,
) -> Result<bool, OrchestratorError> {
    if !super::super::identity::is_git_sha(before) || !super::super::identity::is_git_sha(after) {
        return Err(internal("rolling_ancestry_sha"));
    }
    if before == after {
        return Ok(true);
    }
    // Files are included only on the first comparison page. Page two keeps
    // this ancestry-only response bounded even for a long legacy history.
    let comparison: Comparison =
        api.get_json(&api.route(&format!("compare/{before}...{after}?per_page=1&page=2")))?;
    Ok(comparison.base_commit.sha == before
        && comparison.merge_base_commit.sha == before
        && comparison.behind_by == 0
        && comparison.status == "ahead"
        && comparison.ahead_by > 0)
}

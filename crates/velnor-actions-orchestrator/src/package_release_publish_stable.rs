// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{GitHubPackagePublisher, OrchestratorError, github::ReleaseResponse, internal};
use velnor_actions_contract::package_release::PackageReleaseChannel;
use velnor_actions_contract::publication_retry::{
    PublicationAction, PublicationState, publication_action,
};

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    id: u64,
    tag: String,
    body: Option<String>,
    draft: Option<bool>,
    prerelease: Option<bool>,
    immutable: Option<bool>,
    assets: Vec<(u64, String, u64, Option<String>)>,
}

pub(in crate::package_release_publish) struct Observation {
    state: PublicationState,
    release: Option<Snapshot>,
}

pub(super) enum Action<'a> {
    Tag,
    Draft,
    Upload(&'a str),
    Publish,
}

impl Snapshot {
    fn new(release: &ReleaseResponse) -> Self {
        let mut assets = release
            .assets
            .iter()
            .map(|a| (a.id, a.name.clone(), a.size, a.digest.clone()))
            .collect::<Vec<_>>();
        assets.sort();
        Self {
            id: release.id,
            tag: release.tag_name.clone(),
            body: release.body.clone(),
            draft: release.draft,
            prerelease: release.prerelease,
            immutable: release.immutable,
            assets,
        }
    }
}

pub(super) fn capture(state: &PublicationState, release: Option<&ReleaseResponse>) -> Observation {
    Observation {
        state: state.clone(),
        release: release.map(Snapshot::new),
    }
}

pub(super) fn take(
    publisher: &mut GitHubPackagePublisher,
    requested: &Action<'_>,
) -> Result<Observation, OrchestratorError> {
    take_observation(
        &mut publisher.stable_observation,
        &publisher.identity,
        requested,
    )
}

fn take_observation(
    pending: &mut Option<Observation>,
    identity: &velnor_actions_contract::publication_retry::PublicationIdentity,
    requested: &Action<'_>,
) -> Result<Observation, OrchestratorError> {
    let observed = pending
        .take()
        .ok_or_else(|| internal("stable_write_observation_missing"))?;
    let available = identity.assets.keys().cloned().collect();
    let planned = publication_action(identity, &observed.state, true, &available);
    let matches = match (requested, &planned) {
        (Action::Tag, PublicationAction::CreateTag)
        | (Action::Draft, PublicationAction::CreateDraft) => observed.release.is_none(),
        (Action::Upload(name), PublicationAction::CompleteDraft { missing_assets }) => {
            missing_assets.first().is_some_and(|first| first == name) && observed.release.is_some()
        }
        (Action::Publish, PublicationAction::CompleteDraft { missing_assets }) => {
            missing_assets.is_empty() && observed.release.is_some()
        }
        _ => false,
    };
    if !matches {
        return Err(internal("stable_write_action_changed"));
    }
    Ok(observed)
}

pub(super) fn authorize(
    publisher: &mut GitHubPackagePublisher,
    observed: Observation,
    require_absent_tag: bool,
) -> Result<Option<u64>, OrchestratorError> {
    let id = if let Some(expected) = observed.release {
        refresh_snapshot(&publisher.api, &expected)?;
        Some(expected.id)
    } else {
        if publisher.release_by_tag()?.is_some() {
            return Err(internal("stable_write_release_appeared"));
        }
        None
    };
    if !crate::package_publication::PackagePublicationBackend::candidate_current(publisher)? {
        return Err(internal("package_publication_refused:StaleCandidate"));
    }
    if publisher.receipt.channel == PackageReleaseChannel::Preview {
        let tag = publisher.api.tag_identity(&publisher.tag)?;
        authorize_preview_tag(
            tag,
            &publisher.identity.source_sha,
            &publisher.recovery_body,
            require_absent_tag,
        )?;
    }
    Ok(id)
}

fn authorize_preview_tag(
    tag: Option<super::super::github::TagObservation>,
    source_sha: &str,
    recovery_body: &str,
    require_absent: bool,
) -> Result<(), OrchestratorError> {
    if require_absent {
        return if tag.is_none() {
            Ok(())
        } else {
            Err(internal("preview_write_tag_appeared"))
        };
    }
    if tag.is_some_and(|tag| {
        tag.source_sha == source_sha && tag.message.as_deref() == Some(recovery_body)
    }) {
        Ok(())
    } else {
        Err(internal("preview_write_tag_changed"))
    }
}

fn refresh_snapshot(
    api: &super::super::GitHubApi,
    expected: &Snapshot,
) -> Result<(), OrchestratorError> {
    if expected.id == 0 {
        return Err(internal("stable_write_release_changed"));
    }
    let mut fresh: ReleaseResponse =
        api.get_json(&api.route(&format!("releases/{}", expected.id)))?;
    fresh.assets = api.complete_release_assets(expected.id)?;
    if Snapshot::new(&fresh) != *expected {
        return Err(internal("stable_write_release_changed"));
    }
    Ok(())
}

pub(super) fn verify_current_ci(
    publisher: &GitHubPackagePublisher,
) -> Result<(), OrchestratorError> {
    let receipt = &publisher.receipt;
    let source = super::super::run_identity::RunSource {
        repository_id: receipt.repository.id,
        sha: &receipt.source.commit_sha,
        branch: &receipt.source.default_branch,
    };
    publisher.api.verify_ci_identity(
        receipt.source.ci_run_id,
        receipt.source.ci_run_attempt,
        source,
        false,
    )?;
    let workflow = publisher.api.admitted_workflow(
        velnor_actions_contract::CI_WORKFLOW_PATH,
        velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
    )?;
    let current = publisher.api.get_json(
        &publisher
            .api
            .route(&format!("actions/runs/{}", receipt.source.ci_run_id)),
    )?;
    if !super::super::run_identity::ci_run_is_success(
        &current,
        receipt.source.ci_run_id,
        receipt.source.ci_run_attempt,
        source,
        &workflow,
    ) {
        return Err(internal("stable_current_source_ci_changed"));
    }
    Ok(())
}

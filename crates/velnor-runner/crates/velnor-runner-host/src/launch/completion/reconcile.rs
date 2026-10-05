//! Idempotent completion cleanup and archive lease recovery.

use crate::action_archive_seed::{ActionArchiveLease, ActionArchiveStore};
use crate::journal::Journal;
use crate::reconcile::IntentRow;
use crate::scale_set::EnsureError;
use crate::stage::{ObservedWorker, PairEngine, cleanup_worker, reconcile_worker};
use velnor_runner_github::{Transport, get_runner_by_name, remove_runner};

use super::support::{held, map_journal, open_archive_store, runner_matches, unix_seconds};

pub(super) fn archive_store_for(
    journal: &Journal,
    row: &IntentRow,
) -> Result<Option<ActionArchiveStore>, &'static str> {
    if row.seed_generation_id.is_some() {
        open_archive_store(journal)
            .map(Some)
            .map_err(|_| "archive-store-unavailable")
    } else {
        Ok(None)
    }
}

pub(super) fn open_archive_lease(
    store: Option<&ActionArchiveStore>,
    row: &IntentRow,
    identity: &crate::journal::LaunchIdentity,
) -> Result<Option<ActionArchiveLease>, &'static str> {
    match (store, row.seed_generation_id.as_deref()) {
        (Some(store), Some(generation)) => store
            .open_existing_lease(identity.launch_id(), generation)
            .map(Some)
            .map_err(|_| "archive-lease-unavailable"),
        (None, None) => Ok(None),
        _ => Err("archive-lease-identity"),
    }
}

pub(super) async fn observe_and_bind<E: PairEngine>(
    docker: &E,
    journal: &Journal,
    row: &IntentRow,
    identity: &crate::journal::LaunchIdentity,
    archive_lease: Option<&ActionArchiveLease>,
) -> Result<Option<ObservedWorker>, EnsureError> {
    let observed = match reconcile_worker(
        docker,
        identity,
        row.docker_id.as_deref(),
        row.dind_id.as_deref(),
        archive_lease,
    )
    .await
    {
        Ok(observed) => observed,
        Err(_) => return held(row, "docker-observation").map(|_| None),
    };
    if observed.runner_running() == Some(true) {
        return held(row, "runner-active").map(|_| None);
    }
    if observed.runner_id().is_some() && observed.runner_running() != Some(false) {
        return held(row, "runner-state-unknown").map(|_| None);
    }
    bind_observed_ids(journal, row.id, &observed).await?;
    Ok(Some(observed))
}

async fn bind_observed_ids(
    journal: &Journal,
    id: i64,
    observed: &ObservedWorker,
) -> Result<(), EnsureError> {
    if let Some(runner_id) = observed.runner_id() {
        journal
            .bind_runner_container(id, runner_id)
            .await
            .map_err(map_journal)?;
    }
    if let Some(dind_id) = observed.dind_id() {
        journal
            .bind_dind_container(id, dind_id)
            .await
            .map_err(map_journal)?;
    }
    Ok(())
}

pub(super) fn remove_registered_runner<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    set_id: i64,
    runner_name: &str,
    runner_id: Option<&str>,
) -> Result<(), &'static str> {
    let Some(runner_id) = runner_id.and_then(|id| id.parse::<i64>().ok()) else {
        return Err("github-id-missing");
    };
    match get_runner_by_name(transport, runner_name, admin_token) {
        Ok(Some(runner)) if runner_matches(&runner, runner_id, set_id, runner_name) => {
            remove_runner(transport, runner.id, admin_token).map_err(|_| "github-delete")?;
            match get_runner_by_name(transport, runner_name, admin_token) {
                Ok(None) => Ok(()),
                Ok(Some(_)) => Err("github-still-present"),
                Err(_) => Err("github-confirm-absence"),
            }
        }
        Ok(None) => Ok(()),
        Ok(Some(_)) => Err("github-identity"),
        Err(_) => Err("github-lookup"),
    }
}

pub(super) async fn finish_proven_completion(
    journal: &Journal,
    row: &IntentRow,
    identity: &crate::journal::LaunchIdentity,
    archive_store: Option<&ActionArchiveStore>,
    claim_generation: i64,
) -> Result<bool, EnsureError> {
    if let Err(stage) = retire_archive_lease(archive_store, row, identity) {
        return held(row, stage);
    }
    finish_completion_cleanup(journal, row, claim_generation).await
}

fn retire_archive_lease(
    store: Option<&ActionArchiveStore>,
    row: &IntentRow,
    identity: &crate::journal::LaunchIdentity,
) -> Result<(), &'static str> {
    match (store, row.seed_generation_id.as_deref()) {
        (Some(store), Some(generation)) => store
            .release_after_confirmed_cleanup(identity.launch_id(), generation)
            .map_err(|_| "archive-lease-retirement"),
        (None, None) => Ok(()),
        _ => Err("archive-lease-identity"),
    }
}

async fn finish_completion_cleanup(
    journal: &Journal,
    row: &IntentRow,
    claim_generation: i64,
) -> Result<bool, EnsureError> {
    let completed = journal
        .record_completion_cleanup(row.id, claim_generation, unix_seconds()?)
        .await
        .map_err(map_journal)?;
    if completed {
        Ok(true)
    } else {
        held(row, "cleanup-claim-expired")
    }
}

pub(super) async fn cleanup_owned_pair<E: PairEngine>(
    docker: &E,
    identity: &crate::journal::LaunchIdentity,
    observed: &ObservedWorker,
    row: &IntentRow,
    archive_lease: Option<&ActionArchiveLease>,
) -> Result<(), &'static str> {
    cleanup_worker(
        docker,
        identity,
        observed.runner_id().or(row.docker_id.as_deref()),
        observed.dind_id().or(row.dind_id.as_deref()),
        archive_lease,
    )
    .await
    .map_err(|_| "docker-cleanup")
}

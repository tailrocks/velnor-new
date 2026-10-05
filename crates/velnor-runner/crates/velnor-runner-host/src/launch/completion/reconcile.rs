//! Idempotent completion cleanup and archive lease recovery.

use crate::action_archive_seed::{ActionArchiveLease, ActionArchiveStore};
use crate::journal::Journal;
use crate::reconcile::IntentRow;
use crate::scale_set::EnsureError;
use crate::stage::{ObservedWorker, PairEngine, reconcile_worker};
use velnor_runner_github::{Transport, get_runner_by_name, remove_runner};

#[path = "completion_effects.rs"]
mod completion_effects;
use completion_effects::cleanup_owned_pair;

use super::support::{held, map_journal, open_archive_store, runner_matches};

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
    claim_generation: i64,
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
    if !bind_observed_ids(journal, row.id, claim_generation, &observed).await? {
        return held(row, "cleanup-container-binding").map(|_| None);
    }
    Ok(Some(observed))
}

async fn bind_observed_ids(
    journal: &Journal,
    id: i64,
    claim_generation: i64,
    observed: &ObservedWorker,
) -> Result<bool, EnsureError> {
    journal
        .bind_completion_containers(
            id,
            claim_generation,
            observed.runner_id(),
            observed.dind_id(),
        )
        .await
        .map_err(map_journal)
}

pub(super) async fn remove_registered_runner<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    set_id: i64,
    runner_name: &str,
    runner_id: Option<&str>,
    journal: &Journal,
    row_id: i64,
    claim_generation: i64,
) -> Result<(), &'static str> {
    let Some(runner_id) = runner_id.and_then(|id| id.parse::<i64>().ok()) else {
        return Err("github-id-missing");
    };
    if !current_claim(journal, row_id, claim_generation).await? {
        return Err("cleanup-claim-expired");
    }
    let found = get_runner_by_name(transport, runner_name, admin_token);
    if !current_claim(journal, row_id, claim_generation).await? {
        return Err("cleanup-claim-expired");
    }
    match found {
        Ok(Some(runner)) if runner_matches(&runner, runner_id, set_id, runner_name) => {
            let id = runner.id;
            let deleted = journal
                .run_completion_cleanup_effect(row_id, claim_generation, || async {
                    Ok(remove_runner(transport, id, admin_token).map_err(|_| "github-delete"))
                })
                .await
                .map_err(|_| "cleanup-claim-check")?;
            match deleted {
                Some(Ok(())) => {}
                Some(Err(stage)) => return Err(stage),
                None => return Err("cleanup-claim-expired"),
            }
            if !current_claim(journal, row_id, claim_generation).await? {
                return Err("cleanup-claim-expired");
            }
            let absent = get_runner_by_name(transport, runner_name, admin_token);
            if !current_claim(journal, row_id, claim_generation).await? {
                return Err("cleanup-claim-expired");
            }
            match absent {
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

async fn current_claim(
    journal: &Journal,
    row_id: i64,
    claim_generation: i64,
) -> Result<bool, &'static str> {
    journal
        .completion_cleanup_claim_current(row_id, claim_generation)
        .await
        .map_err(|_| "cleanup-claim-check")
}

pub(super) async fn finish_proven_completion(
    journal: &Journal,
    row: &IntentRow,
    identity: &crate::journal::LaunchIdentity,
    archive_store: Option<&ActionArchiveStore>,
    claim_generation: i64,
) -> Result<bool, EnsureError> {
    if !claim_is_current(journal, row.id, claim_generation).await? {
        return held(row, "cleanup-claim-expired");
    }
    let retired = journal
        .run_completion_cleanup_effect(row.id, claim_generation, || async {
            Ok(retire_archive_lease(archive_store, row, identity))
        })
        .await
        .map_err(map_journal)?;
    let Some(retired) = retired else {
        return held(row, "cleanup-claim-expired");
    };
    if let Err(stage) = retired {
        return held(row, stage);
    }
    if !claim_is_current(journal, row.id, claim_generation).await? {
        return held(row, "cleanup-claim-expired");
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
        .record_completion_cleanup(row.id, claim_generation)
        .await
        .map_err(map_journal)?;
    if completed {
        Ok(true)
    } else {
        held(row, "cleanup-claim-expired")
    }
}

pub(super) async fn reconcile_one<T: Transport + ?Sized, E: PairEngine>(
    transport: &mut T,
    set_id: i64,
    admin_token: &str,
    journal: &Journal,
    docker: &E,
    claim_generation: i64,
    row: IntentRow,
) -> Result<bool, EnsureError> {
    if !claim_is_current(journal, row.id, claim_generation).await? {
        return held(&row, "cleanup-claim-expired");
    }
    let identity = journal.launch_identity(row.id).await.map_err(map_journal)?;
    let archive_store = match archive_store_for(journal, &row) {
        Ok(store) => store,
        Err(stage) => return held(&row, stage),
    };
    if journal
        .completion_worker_cleanup_proven(row.id)
        .await
        .map_err(map_journal)?
    {
        return finish_proven_completion(
            journal,
            &row,
            &identity,
            archive_store.as_ref(),
            claim_generation,
        )
        .await;
    }
    let archive_lease = match open_archive_lease(archive_store.as_ref(), &row, &identity) {
        Ok(lease) => lease,
        Err(stage) => return held(&row, stage),
    };
    reconcile_unproven_completion(
        transport,
        set_id,
        admin_token,
        journal,
        docker,
        claim_generation,
        &row,
        &identity,
        archive_store.as_ref(),
        archive_lease.as_ref(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn reconcile_unproven_completion<T: Transport + ?Sized, E: PairEngine>(
    transport: &mut T,
    set_id: i64,
    admin_token: &str,
    journal: &Journal,
    docker: &E,
    claim_generation: i64,
    row: &IntentRow,
    identity: &crate::journal::LaunchIdentity,
    archive_store: Option<&ActionArchiveStore>,
    archive_lease: Option<&ActionArchiveLease>,
) -> Result<bool, EnsureError> {
    let Some(observed) = observe_current_worker(
        docker,
        journal,
        row,
        identity,
        archive_lease,
        claim_generation,
    )
    .await?
    else {
        return Ok(false);
    };
    let runner_name = format!("v{}", identity.launch_id());
    if let Err(stage) = remove_registered_runner(
        transport,
        admin_token,
        set_id,
        &runner_name,
        row.github_runner_id.as_deref(),
        journal,
        row.id,
        claim_generation,
    )
    .await
    {
        return held(&row, stage);
    }
    if !claim_is_current(journal, row.id, claim_generation).await? {
        return held(&row, "cleanup-claim-expired");
    }
    if let Err(stage) = cleanup_owned_pair(
        journal,
        docker,
        identity,
        &observed,
        row,
        archive_lease,
        claim_generation,
    )
    .await
    {
        return held(&row, stage);
    }
    if !claim_is_current(journal, row.id, claim_generation).await? {
        return held(&row, "cleanup-claim-expired");
    }
    let marked = journal
        .mark_completion_worker_cleanup_proven(row.id, claim_generation)
        .await
        .map_err(map_journal)?;
    if !marked {
        return held(&row, "cleanup-claim-expired");
    }
    finish_proven_completion(journal, row, identity, archive_store, claim_generation).await
}

async fn claim_is_current(
    journal: &Journal,
    row_id: i64,
    generation: i64,
) -> Result<bool, EnsureError> {
    journal
        .completion_cleanup_claim_current(row_id, generation)
        .await
        .map_err(map_journal)
}

async fn observe_current_worker<E: PairEngine>(
    docker: &E,
    journal: &Journal,
    row: &IntentRow,
    identity: &crate::journal::LaunchIdentity,
    archive_lease: Option<&crate::action_archive_seed::ActionArchiveLease>,
    generation: i64,
) -> Result<Option<crate::stage::ObservedWorker>, EnsureError> {
    if !claim_is_current(journal, row.id, generation).await? {
        return Ok(None);
    }
    let observed =
        observe_and_bind(docker, journal, row, identity, archive_lease, generation).await?;
    if !claim_is_current(journal, row.id, generation).await? {
        return Ok(None);
    }
    Ok(observed)
}

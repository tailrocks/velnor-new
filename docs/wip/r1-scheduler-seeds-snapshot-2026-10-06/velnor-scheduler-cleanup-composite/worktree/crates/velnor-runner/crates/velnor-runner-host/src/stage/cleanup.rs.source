//! Bounded cleanup after an official runner has reached a terminal state.

use super::{PairEngine, WorkerVolume, reconcile_worker, reconcile_worker_with_budget};
use crate::action_archive_seed::ActionArchiveLease;
use crate::error::HostError;
use crate::journal::LaunchIdentity;
use crate::worker::ResourceBudget;

/// Remove one worker pair after its runner has stopped.
///
/// The function verifies the daemon, full labels, ids, and deterministic names.
/// It removes private volumes only after both containers are absent.
pub(crate) async fn cleanup_worker<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    runner_id: Option<&str>,
    dind_id: Option<&str>,
    archive_lease: Option<&ActionArchiveLease>,
) -> Result<(), HostError> {
    cleanup_pair(engine, identity, runner_id, dind_id, archive_lease).await
}

pub(super) async fn cleanup_unstarted_dind<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    dind_id: &str,
    resource_budget: ResourceBudget,
) -> Result<(), HostError> {
    let observed =
        reconcile_worker_with_budget(engine, identity, None, Some(dind_id), None, resource_budget)
            .await?;
    if observed.runner_id().is_some() {
        return Err(HostError::Ownership);
    }
    if let Some(id) = observed.dind_id() {
        cleanup_worker_container(engine, identity, "dind", id, None, None).await?;
    }
    let after = reconcile_worker(engine, identity, None, None, None).await?;
    if after.dind_id().is_some() || after.runner_id().is_some() {
        return Err(HostError::Cleanup);
    }
    remove_private_volumes(engine, identity).await
}

pub(super) async fn cleanup_pair<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    runner_id: Option<&str>,
    dind_id: Option<&str>,
    archive_lease: Option<&ActionArchiveLease>,
) -> Result<(), HostError> {
    let observed = reconcile_worker(engine, identity, runner_id, dind_id, archive_lease).await?;
    if let Some(id) = observed.runner_id() {
        cleanup_worker_container(
            engine,
            identity,
            "runner",
            id,
            observed.dind_id(),
            archive_lease,
        )
        .await?;
    }
    let after_runner = reconcile_worker(engine, identity, None, dind_id, None).await?;
    if after_runner.runner_id().is_some() {
        return Err(HostError::Cleanup);
    }
    if let Some(id) = after_runner.dind_id() {
        cleanup_worker_container(engine, identity, "dind", id, None, None).await?;
    }
    let after_pair = reconcile_worker(engine, identity, None, None, None).await?;
    if after_pair.runner_id().is_some() || after_pair.dind_id().is_some() {
        return Err(HostError::Cleanup);
    }
    remove_private_volumes(engine, identity).await
}

/// Remove one exact stopped container after ownership checks.
pub(crate) async fn cleanup_worker_container<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    role: &str,
    id: &str,
    dind_id: Option<&str>,
    archive_lease: Option<&ActionArchiveLease>,
) -> Result<(), HostError> {
    let observed = reconcile_worker(
        engine,
        identity,
        (role == "runner").then_some(id),
        (role == "dind").then_some(id),
        archive_lease,
    )
    .await?;
    verify_container_for_cleanup(role, id, dind_id, archive_lease, &observed)?;
    engine.remove(id).await?;
    if engine.inspect_container(id).await?.is_some() {
        return Err(HostError::Cleanup);
    }
    Ok(())
}

fn verify_container_for_cleanup(
    role: &str,
    id: &str,
    dind_id: Option<&str>,
    archive_lease: Option<&ActionArchiveLease>,
    observed: &super::ObservedWorker,
) -> Result<(), HostError> {
    match role {
        "runner" if observed.runner_id() == Some(id) => {
            if observed.dind_id() != dind_id {
                return Err(HostError::Ownership);
            }
            match observed.runner_running() {
                Some(false) => Ok(()),
                Some(true) => Err(HostError::RunnerActive),
                None => Err(HostError::Ownership),
            }
        }
        "dind" if observed.dind_id() == Some(id) => {
            if observed.runner_id().is_some() || dind_id.is_some() || archive_lease.is_some() {
                return Err(HostError::Ownership);
            }
            Ok(())
        }
        _ => Err(HostError::Ownership),
    }
}

async fn remove_private_volumes<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
) -> Result<(), HostError> {
    for volume in WorkerVolume::CLEANUP_ORDER {
        engine.verify_volume(identity, volume).await?;
        engine.remove_volume(identity, volume).await?;
    }
    Ok(())
}

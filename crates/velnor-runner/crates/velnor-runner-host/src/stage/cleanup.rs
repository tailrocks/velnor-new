//! Bounded cleanup after an official runner has reached a terminal state.

use super::pair::PairEngine;
use super::{reconcile_worker, reconcile_worker_with_budget};
use crate::error::HostError;
use crate::launch_identity::LaunchIdentity;
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
) -> Result<(), HostError> {
    cleanup_pair(engine, identity, runner_id, dind_id).await
}

pub(super) async fn cleanup_unstarted_dind<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    dind_id: &str,
) -> Result<(), HostError> {
    cleanup_unstarted_dind_inner(engine, identity, dind_id, None).await
}

pub(super) async fn cleanup_unstarted_dind_with_budget<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    dind_id: &str,
    resource_budget: ResourceBudget,
) -> Result<(), HostError> {
    cleanup_unstarted_dind_inner(engine, identity, dind_id, Some(resource_budget)).await
}

async fn cleanup_unstarted_dind_inner<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    dind_id: &str,
    resource_budget: Option<ResourceBudget>,
) -> Result<(), HostError> {
    let observed = match resource_budget {
        Some(budget) => {
            reconcile_worker_with_budget(engine, identity, None, Some(dind_id), budget).await?
        }
        None => reconcile_worker(engine, identity, None, Some(dind_id)).await?,
    };
    if observed.runner_id().is_some() {
        return Err(HostError::Ownership);
    }
    if let Some(id) = observed.dind_id() {
        engine.remove(id).await?;
    }
    let after = reconcile_worker(engine, identity, None, None).await?;
    if after.dind_id().is_some() || after.runner_id().is_some() {
        return Err(HostError::Cleanup);
    }
    engine.remove_volumes(identity).await
}

pub(super) async fn cleanup_pair<E: PairEngine>(
    engine: &E,
    identity: &LaunchIdentity,
    runner_id: Option<&str>,
    dind_id: Option<&str>,
) -> Result<(), HostError> {
    let observed = reconcile_worker(engine, identity, runner_id, dind_id).await?;
    if let Some(id) = observed.runner_id() {
        let runner = engine
            .inspect_container(id)
            .await?
            .ok_or(HostError::Ownership)?;
        if runner.running != Some(false) {
            return Err(if runner.running == Some(true) {
                HostError::RunnerActive
            } else {
                HostError::Ownership
            });
        }
        engine.remove(id).await?;
    }
    let after_runner = reconcile_worker(engine, identity, None, dind_id).await?;
    if after_runner.runner_id().is_some() {
        return Err(HostError::Cleanup);
    }
    if let Some(id) = after_runner.dind_id() {
        engine.remove(id).await?;
    }
    let after_pair = reconcile_worker(engine, identity, None, None).await?;
    if after_pair.runner_id().is_some() || after_pair.dind_id().is_some() {
        return Err(HostError::Cleanup);
    }
    engine.remove_volumes(identity).await
}

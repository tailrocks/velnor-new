//! Bounded cleanup after an official runner has reached a terminal state.

use super::{PairEngine, reconcile_worker};
use crate::action_archive_seed::ActionArchiveLease;
use crate::error::HostError;
use crate::journal::LaunchIdentity;

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
) -> Result<(), HostError> {
    let observed = reconcile_worker(engine, identity, None, Some(dind_id), None).await?;
    if observed.runner_id().is_some() {
        return Err(HostError::Ownership);
    }
    if let Some(id) = observed.dind_id() {
        engine.remove(id).await?;
    }
    let after = reconcile_worker(engine, identity, None, None, None).await?;
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
    archive_lease: Option<&ActionArchiveLease>,
) -> Result<(), HostError> {
    let observed = reconcile_worker(engine, identity, runner_id, dind_id, archive_lease).await?;
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
    let after_runner = reconcile_worker(engine, identity, None, dind_id, None).await?;
    if after_runner.runner_id().is_some() {
        return Err(HostError::Cleanup);
    }
    if let Some(id) = after_runner.dind_id() {
        engine.remove(id).await?;
    }
    let after_pair = reconcile_worker(engine, identity, None, None, None).await?;
    if after_pair.runner_id().is_some() || after_pair.dind_id().is_some() {
        return Err(HostError::Cleanup);
    }
    engine.remove_volumes(identity).await
}

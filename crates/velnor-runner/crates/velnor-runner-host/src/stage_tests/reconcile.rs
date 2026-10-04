//! Recovery tests for containers created before a lost response or restart.

use super::fake::Fake;
use super::identity;
use crate::error::HostError;
use crate::stage::PairEngine;
use crate::stage::{prepare_dind, reconcile_worker, start_runner};
use crate::worker::dind_create;

#[tokio::test]
async fn lost_dind_create_response_is_recovered_by_launch_identity() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    *engine
        .lose_create_response_at
        .lock()
        .map_err(|_| HostError::Docker)? = Some(1);
    assert_eq!(
        prepare_dind(&engine, &identity).await,
        Err(HostError::ContainerCreateUncertain)
    );

    let observed = reconcile_worker(&engine, &identity, None, None, None).await?;
    let dind_id = observed.dind_id().ok_or(HostError::Ownership)?;
    assert!(observed.runner_id().is_none());
    let prepared = prepare_dind(&engine, &identity).await?;
    assert_eq!(prepared.dind_id(), dind_id);
    assert_eq!(engine.containers()?, 1);
    start_runner(&engine, &prepared, b"jit", None).await?;
    Ok(())
}

#[tokio::test]
async fn empty_rows_reconcile_as_absent_and_stale_recorded_id_is_checked() -> Result<(), HostError>
{
    let identity = identity()?;
    let engine = Fake::new();
    let absent = reconcile_worker(&engine, &identity, None, None, None).await?;
    assert!(absent.dind_id().is_none());
    assert!(absent.runner_id().is_none());

    let gone_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    let absent = reconcile_worker(&engine, &identity, None, Some(gone_id), None).await?;
    assert!(absent.dind_id().is_none());
    Ok(())
}

#[tokio::test]
async fn duplicate_exact_role_rows_are_not_adopted() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    engine.create(&dind_create(&identity)?).await?;
    engine.create(&dind_create(&identity)?).await?;
    assert_eq!(
        reconcile_worker(&engine, &identity, None, None, None).await,
        Err(HostError::Ownership)
    );
    Ok(())
}

#[tokio::test]
async fn recorded_id_conflict_is_not_adopted() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity).await?;
    let other_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    assert_eq!(
        reconcile_worker(&engine, &identity, None, Some(other_id), None).await,
        Err(HostError::Ownership)
    );
    assert_eq!(
        reconcile_worker(&engine, &identity, None, Some(prepared.dind_id()), None)
            .await?
            .dind_id(),
        Some(prepared.dind_id())
    );
    Ok(())
}

#[tokio::test]
async fn docker_inspect_failure_is_not_absence() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    *engine.inspect_error.lock().map_err(|_| HostError::Docker)? = Some(HostError::DockerTimeout);
    let recorded = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    assert_eq!(
        reconcile_worker(&engine, &identity, None, Some(recorded), None).await,
        Err(HostError::DockerTimeout)
    );
    Ok(())
}

#[tokio::test]
async fn runner_without_its_exact_dind_is_uncertain() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity).await?;
    let started = start_runner(&engine, &prepared, b"jit", None).await?;
    engine.remove(&started.dind_id).await?;
    assert_eq!(
        reconcile_worker(&engine, &identity, None, None, None,).await,
        Err(HostError::Ownership)
    );
    Ok(())
}

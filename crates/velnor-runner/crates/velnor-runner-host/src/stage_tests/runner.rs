//! Runner creation follows durable `DinD` readiness.

use super::fake::Fake;
use super::identity;
use crate::error::HostError;
use crate::stage::{prepare_dind, reconcile_worker, start_runner};
use crate::worker::{PreparedDind, Started};

async fn ready_pair(engine: &Fake) -> Result<(PreparedDind, Started), HostError> {
    let identity = identity()?;
    let prepared = prepare_dind(engine, &identity).await?;
    let started = start_runner(engine, &prepared, b"jit").await?;
    Ok((prepared, started))
}

#[tokio::test]
async fn runner_start_uses_only_a_verified_prepared_dind() -> Result<(), HostError> {
    let engine = Fake::new();
    let (prepared, started) = ready_pair(&engine).await?;
    assert_eq!(started.dind_id, prepared.dind_id());
    let observed = reconcile_worker(
        &engine,
        &identity()?,
        Some(&started.runner_id),
        Some(&started.dind_id),
    )
    .await?;
    assert_eq!(observed.runner_id(), Some(started.runner_id.as_str()));
    assert_eq!(observed.dind_id(), Some(started.dind_id.as_str()));
    assert_eq!(observed.runner_running(), Some(true));
    let events = engine.events()?;
    assert!(events.windows(2).any(|pair| pair == ["volumes", "create"]));
    assert!(events.windows(2).any(|pair| pair == ["start", "probe"]));
    assert!(
        events
            .windows(2)
            .any(|pair| pair == ["verify-dind", "create"])
    );
    assert!(events.windows(2).any(|pair| pair == ["start", "jit"]));
    Ok(())
}

#[tokio::test]
async fn journal_recovery_rechecks_the_full_dind_identity() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity).await?;
    let recovered = PreparedDind::from_journal(&identity, prepared.dind_id())?;
    start_runner(&engine, &recovered, b"jit").await?;
    assert!(engine.events()?.contains(&"verify-dind"));
    Ok(())
}

#[tokio::test]
async fn recovered_handle_cannot_relabel_a_foreign_dind() -> Result<(), HostError> {
    let identity = identity()?;
    let foreign = crate::launch_identity::LaunchIdentity::new(
        "cccccccccccccccccccccccccccccccc",
        8,
        "dddddddddddddddddddddddddddddddd",
        "engine-test",
    )?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity).await?;
    let forged = PreparedDind::from_journal(&foreign, prepared.dind_id())?;
    assert_eq!(
        start_runner(&engine, &forged, b"jit").await,
        Err(HostError::Ownership)
    );
    assert_eq!(engine.containers()?, 1);
    assert!(!engine.events()?.contains(&"jit"));
    Ok(())
}

#[tokio::test]
async fn runner_start_failure_keeps_pair_for_reconciliation() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity).await?;
    *engine.fail_start_at.lock().map_err(|_| HostError::Docker)? = Some(2);
    assert_eq!(
        start_runner(&engine, &prepared, b"jit").await,
        Err(HostError::ContainerStartUncertain)
    );
    assert_eq!(engine.containers()?, 2);
    assert!(!engine.events()?.contains(&"remove"));
    Ok(())
}

#[tokio::test]
async fn jit_delivery_failure_keeps_pair_for_reconciliation() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity).await?;
    *engine.fail_jit.lock().map_err(|_| HostError::Docker)? = true;
    assert_eq!(
        start_runner(&engine, &prepared, b"jit").await,
        Err(HostError::JitDeliveryUncertain)
    );
    assert_eq!(engine.containers()?, 2);
    assert!(!engine.events()?.contains(&"remove"));
    Ok(())
}

#[tokio::test]
async fn lost_runner_create_response_keeps_pair_for_reconciliation() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity).await?;
    *engine
        .lose_create_response_at
        .lock()
        .map_err(|_| HostError::Docker)? = Some(2);
    assert_eq!(
        start_runner(&engine, &prepared, b"jit").await,
        Err(HostError::ContainerCreateUncertain)
    );
    assert_eq!(engine.containers()?, 2);
    assert!(!engine.events()?.contains(&"remove"));
    assert!(!engine.events()?.contains(&"jit"));
    Ok(())
}

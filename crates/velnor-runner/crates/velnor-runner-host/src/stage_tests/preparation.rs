//! Readiness and pre-JIT cleanup tests.

use super::fake::Fake;
use super::identity;
use crate::error::{HostError, PreparationCause};
use crate::stage::{DindProbe, prepare_dind};

#[tokio::test]
async fn prepare_waits_for_inner_api_before_returning() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    engine
        .probes
        .lock()
        .map_err(|_| HostError::Docker)?
        .extend([Ok(DindProbe::Starting), Ok(DindProbe::Ready)]);
    let prepared = prepare_dind(&engine, &identity).await?;
    assert_eq!(prepared.dind_id().len(), 64);
    assert_eq!(engine.containers()?, 1);
    let events = engine.events()?;
    assert!(events.windows(2).any(|pair| pair == ["volumes", "create"]));
    assert!(events.windows(2).any(|pair| pair == ["start", "probe"]));
    assert_eq!(events.iter().filter(|event| **event == "probe").count(), 2);
    Ok(())
}

#[tokio::test]
async fn readiness_failure_returns_typed_error_after_confirmed_cleanup() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    engine
        .probes
        .lock()
        .map_err(|_| HostError::Docker)?
        .push_back(Err(HostError::DindStorage));
    assert_eq!(
        prepare_dind(&engine, &identity).await,
        Err(HostError::PreparationFailedClean(
            PreparationCause::DindStorage
        ))
    );
    assert_eq!(engine.containers()?, 0);
    assert!(engine.events()?.ends_with(&["remove", "remove-volumes"]));
    Ok(())
}

#[tokio::test]
async fn confirmed_cleanup_preserves_each_readiness_cause() -> Result<(), HostError> {
    for (error, cause) in [
        (HostError::Docker, PreparationCause::Docker),
        (HostError::DockerTimeout, PreparationCause::DockerTimeout),
        (HostError::DindReadiness, PreparationCause::DindReadiness),
        (HostError::DindStorage, PreparationCause::DindStorage),
    ] {
        let identity = identity()?;
        let engine = Fake::new();
        engine
            .probes
            .lock()
            .map_err(|_| HostError::Docker)?
            .push_back(Err(error));
        assert_eq!(
            prepare_dind(&engine, &identity).await,
            Err(HostError::PreparationFailedClean(cause))
        );
        assert_eq!(engine.containers()?, 0);
    }
    Ok(())
}

#[tokio::test]
async fn readiness_failure_with_container_cleanup_error_stays_uncertain() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    engine
        .probes
        .lock()
        .map_err(|_| HostError::Docker)?
        .push_back(Err(HostError::DindReadiness));
    *engine.fail_remove.lock().map_err(|_| HostError::Docker)? = true;
    assert_eq!(
        prepare_dind(&engine, &identity).await,
        Err(HostError::Cleanup)
    );
    assert_eq!(engine.containers()?, 1);
    assert!(!engine.events()?.contains(&"remove-volumes"));
    Ok(())
}

#[tokio::test]
async fn readiness_failure_with_volume_cleanup_error_stays_uncertain() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    engine
        .probes
        .lock()
        .map_err(|_| HostError::Docker)?
        .push_back(Err(HostError::DockerTimeout));
    *engine.fail_volumes.lock().map_err(|_| HostError::Docker)? = true;
    assert_eq!(
        prepare_dind(&engine, &identity).await,
        Err(HostError::Cleanup)
    );
    assert_eq!(engine.containers()?, 0);
    assert!(engine.events()?.ends_with(&["remove", "remove-volumes"]));
    Ok(())
}

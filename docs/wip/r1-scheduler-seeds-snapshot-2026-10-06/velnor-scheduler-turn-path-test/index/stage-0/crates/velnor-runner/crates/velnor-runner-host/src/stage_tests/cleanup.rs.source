//! Exact-owner cleanup tests.

use bollard::errors::Error as DockerError;

use super::fake::Fake;
use super::identity;
use super::resource_budget;
use crate::error::HostError;
use crate::stage::{cleanup_prepared_dind, cleanup_worker, prepare_dind, start_runner};

#[tokio::test]
async fn prepared_dind_cleanup_removes_only_its_private_resources() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity, resource_budget()?).await?;
    cleanup_prepared_dind(&engine, &prepared).await?;
    assert_eq!(engine.containers()?, 0);
    assert!(
        engine
            .events()?
            .ends_with(&["remove", "engine", "list-launch", "remove-volumes"])
    );
    Ok(())
}

#[tokio::test]
async fn prepared_cleanup_refuses_a_present_runner() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity, resource_budget()?).await?;
    start_runner(&engine, &prepared, b"jit", None).await?;
    assert_eq!(
        cleanup_prepared_dind(&engine, &prepared).await,
        Err(HostError::Ownership)
    );
    assert_eq!(engine.containers()?, 2);
    assert!(!engine.events()?.contains(&"remove-volumes"));
    Ok(())
}

#[tokio::test]
async fn active_runner_cleanup_preserves_pair_and_volumes() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity, resource_budget()?).await?;
    let started = start_runner(&engine, &prepared, b"jit", None).await?;
    assert_eq!(
        cleanup_worker(
            &engine,
            &identity,
            Some(&started.runner_id),
            Some(&started.dind_id),
            None,
        )
        .await,
        Err(HostError::RunnerActive)
    );
    assert_eq!(engine.containers()?, 2);
    assert!(!engine.events()?.contains(&"remove-volumes"));
    Ok(())
}

#[tokio::test]
async fn cleanup_error_keeps_remaining_pair_and_volumes() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity, resource_budget()?).await?;
    let started = start_runner(&engine, &prepared, b"jit", None).await?;
    engine.stop(&started.runner_id)?;
    *engine.fail_remove.lock().map_err(|_| HostError::Docker)? = true;
    assert_eq!(
        cleanup_worker(
            &engine,
            &identity,
            Some(&started.runner_id),
            Some(&started.dind_id),
            None,
        )
        .await,
        Err(HostError::Docker)
    );
    assert_eq!(engine.containers()?, 2);
    assert!(!engine.events()?.contains(&"remove-volumes"));
    Ok(())
}

#[tokio::test]
async fn successful_cleanup_removes_containers_before_volumes() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity, resource_budget()?).await?;
    let started = start_runner(&engine, &prepared, b"jit", None).await?;
    engine.stop(&started.runner_id)?;
    cleanup_worker(
        &engine,
        &identity,
        Some(&started.runner_id),
        Some(&started.dind_id),
        None,
    )
    .await?;
    assert_eq!(engine.containers()?, 0);
    assert_eq!(engine.events()?.last(), Some(&"remove-volumes"));
    Ok(())
}

#[tokio::test]
async fn cleanup_discovers_owned_pair_when_journal_ids_are_missing() -> Result<(), HostError> {
    let identity = identity()?;
    let engine = Fake::new();
    let prepared = prepare_dind(&engine, &identity, resource_budget()?).await?;
    let started = start_runner(&engine, &prepared, b"jit", None).await?;
    engine.stop(&started.runner_id)?;
    cleanup_worker(&engine, &identity, None, None, None).await?;
    assert_eq!(engine.containers()?, 0);
    assert_eq!(engine.events()?.last(), Some(&"remove-volumes"));
    Ok(())
}

#[test]
fn only_a_confirmed_not_found_is_absence() {
    assert!(crate::worker::confirmed_not_found(
        &DockerError::DockerResponseServerError {
            status_code: 404,
            message: "missing".to_owned(),
        }
    ));
    assert!(!crate::worker::confirmed_not_found(
        &DockerError::RequestTimeoutError
    ));
    for status_code in [401, 500, 503] {
        assert!(!crate::worker::confirmed_not_found(
            &DockerError::DockerResponseServerError {
                status_code,
                message: "failure".to_owned(),
            }
        ));
    }
}

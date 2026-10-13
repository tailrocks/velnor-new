use tempfile::TempDir;

use crate::HostError;
use crate::worker::cleanup::*;

use super::fixtures::{FakeEngine, FakeLedger, diagnostics_store, identity};

#[tokio::test]
async fn not_run_requires_durable_never_started_observation() -> Result<(), String> {
    let engine = FakeEngine::never_started();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;

    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::NotRun,
        RunnerStopPolicy::RequireStopped,
    )
    .await
    .map_err(|error| error.to_string())?;

    assert!(proof.diagnostics_source_absent());
    assert_eq!(proof.cleanup_disposition(), CleanupDisposition::Completed);
    assert_eq!(
        ledger
            .runner_start_observation(&identity(None)?)
            .await
            .map_err(|error| error.to_string())?,
        Some(velnor_runner_journal::journal::RunnerStartObservation::NeverStarted)
    );
    Ok(())
}

#[tokio::test]
async fn started_runner_without_lifecycle_event_cannot_be_classified_not_run() -> Result<(), String>
{
    let engine = FakeEngine::running();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;

    let result = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::NotRun,
        RunnerStopPolicy::RequireStopped,
    )
    .await;

    assert_eq!(result, Err(HostError::Identity));
    assert_eq!(engine.events(), Vec::<String>::new());
    assert!(!ledger.completed());
    Ok(())
}

#[tokio::test]
async fn absent_runner_without_start_checkpoint_stays_unresolved() -> Result<(), String> {
    let engine = FakeEngine::absent_runner_without_diagnostics();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;

    let result = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::NotRun,
        RunnerStopPolicy::RequireStopped,
    )
    .await;

    assert_eq!(result, Err(HostError::Identity));
    assert_eq!(engine.events(), Vec::<String>::new());
    assert!(!ledger.completed());
    Ok(())
}

#[tokio::test]
async fn never_started_private_dind_has_durable_empty_child_inventory() -> Result<(), String> {
    let engine = FakeEngine::never_started_dind();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;

    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::NotRun,
        RunnerStopPolicy::RequireStopped,
    )
    .await
    .map_err(|error| error.to_string())?;

    assert_eq!(proof.children().container_ids(), &[] as &[String]);
    assert_eq!(proof.children().network_ids(), &[] as &[String]);
    assert!(!engine.events().contains(&"children-empty".to_owned()));
    assert!(ledger.completed());
    Ok(())
}

#[tokio::test]
async fn stopped_private_dind_that_may_have_started_stays_unresolved() -> Result<(), String> {
    let engine = FakeEngine::stopped_after_dind_start();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;

    let result = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await;

    assert_eq!(result, Err(HostError::Docker));
    let events = engine.events();
    assert!(!events.iter().any(|event| event.starts_with("remove-")));
    assert!(!ledger.completed());
    Ok(())
}

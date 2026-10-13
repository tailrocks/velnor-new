use tempfile::TempDir;

use crate::HostError;
use crate::worker::cleanup::*;

use super::fixtures::{
    CHILD_ID, FakeEngine, FakeLedger, NETWORK_ID, RUNNER_ID, diagnostics_store, identity, index,
};

#[tokio::test]
async fn already_stopped_runner_gets_durable_termination_checkpoint() -> Result<(), String> {
    let engine = FakeEngine::running();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;

    cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        identity(None)?,
        PostActionDisposition::Completed,
        RunnerStopPolicy::RequireStopped,
    )
    .await
    .map_err(|error| error.to_string())?;

    assert!(ledger.has_step("before-RunnerTermination"));
    assert!(ledger.has_step("after-RunnerTermination"));
    assert!(
        !engine
            .events()
            .iter()
            .any(|event| event.starts_with("stop-runner-"))
    );
    Ok(())
}

#[tokio::test]
async fn drained_checkpoint_recovers_when_dind_remains_before_outer_removal() -> Result<(), String>
{
    let engine = FakeEngine::running_with_outer_removal_failure();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;
    let generation = identity(None)?;

    let interrupted = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation.clone(),
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await;
    assert_eq!(interrupted, Err(HostError::Docker));
    assert!(ledger.has_drained_checkpoint());
    assert!(!engine.is_absent(OuterContainerRole::Dind));
    assert!(!engine.is_absent(OuterContainerRole::Runner));

    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation,
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await
    .map_err(|error| error.to_string())?;

    assert_eq!(proof.children().container_ids(), &[CHILD_ID.to_owned()]);
    assert_eq!(proof.children().network_ids(), &[NETWORK_ID.to_owned()]);
    assert!(engine.is_absent(OuterContainerRole::Runner));
    assert!(engine.is_absent(OuterContainerRole::Dind));
    assert_eq!(
        engine
            .events()
            .iter()
            .filter(|event| event.starts_with("remove-child-"))
            .count(),
        2
    );
    assert!(!proof.post_actions_completed());
    Ok(())
}

#[tokio::test]
async fn new_child_after_drain_checkpoint_keeps_outer_resources() -> Result<(), String> {
    let engine = FakeEngine::running_with_dind_stop_failure();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;
    let generation = identity(None)?;

    let interrupted = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation.clone(),
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await;
    assert_eq!(interrupted, Err(HostError::Docker));
    assert!(ledger.has_drained_checkpoint());
    assert!(!ledger.has_step("after-DindTermination"));
    assert!(engine.events().contains(&"stop-dind".to_owned()));
    assert!(!engine.is_absent(OuterContainerRole::Dind));
    assert!(!engine.is_absent(OuterContainerRole::Runner));
    engine.add_child_container("e123456789abcdef");

    let retry = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation,
        PostActionDisposition::Unknown,
        RunnerStopPolicy::RequireStopped,
    )
    .await;

    assert_eq!(retry, Err(HostError::Docker));
    assert!(!engine.is_absent(OuterContainerRole::Dind));
    assert!(!engine.is_absent(OuterContainerRole::Runner));
    assert!(!ledger.completed());
    assert!(
        !engine
            .events()
            .iter()
            .any(|event| event == "remove-child-Container-e123456789abcdef")
    );
    Ok(())
}

#[tokio::test]
async fn lost_runner_stop_response_reconciles_before_child_cleanup() -> Result<(), String> {
    let engine = FakeEngine::running_with_lost_runner_stop_response();
    let ledger = FakeLedger::default();
    let directory = TempDir::new().map_err(|error| error.to_string())?;
    let store = diagnostics_store(&directory)?;
    let generation = identity(None)?;
    let post_actions = PostActionDisposition::Interrupted {
        reason_class: "operator_deadline".to_owned(),
    };
    let stop_policy = RunnerStopPolicy::StopAtDeadline {
        grace_seconds: 1,
        reason_class: "operator_deadline".to_owned(),
    };

    let first = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation.clone(),
        post_actions.clone(),
        stop_policy.clone(),
    )
    .await;
    assert_eq!(first, Err(HostError::Docker));
    assert!(!engine.runner_is_running());
    let first_events = engine.events();
    assert!(first_events.contains(&format!("stop-runner-{RUNNER_ID}")));
    assert!(!first_events.iter().any(|event| {
        event.starts_with("remove-child-")
            || matches!(
                event.as_str(),
                "stop-dind" | "remove-runner" | "remove-dind"
            )
    }));
    assert!(ledger.has_step("before-RunnerTermination"));
    assert!(!ledger.has_step("after-RunnerTermination"));
    assert!(!ledger.completed());

    let proof = cleanup_worker_generation(
        &engine,
        &ledger,
        &store,
        generation,
        post_actions,
        stop_policy,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(proof.physical_capacity_reclaimable());
    assert!(!proof.post_actions_completed());
    let events = engine.events();
    let stop = index(&events, &format!("stop-runner-{RUNNER_ID}"));
    let child = index(&events, "remove-child-");
    let child_empty = index(&events, "children-empty");
    let dind_stop = index(&events, "stop-dind");
    let runner_remove = index(&events, "remove-runner");
    assert!(stop < child && child < child_empty && child_empty < dind_stop);
    assert!(dind_stop < runner_remove);
    let stop_event = format!("stop-runner-{RUNNER_ID}");
    assert_eq!(
        events.iter().filter(|event| *event == &stop_event).count(),
        1
    );
    assert!(ledger.has_step("after-RunnerTermination"));
    assert!(ledger.completed());
    Ok(())
}

use super::stage_tests::{Fake, fake_container_id};
use crate::HostError;
use crate::stage::{PairStartFailure, PairStartPhase, PairStop, drive};
use crate::worker::start_pair_with_sink;

#[tokio::test]
async fn legacy_sink_persists_start_intent_before_runner_start() -> Result<(), PairStartFailure> {
    let engine = Fake::new();
    let started = start_pair_with_sink(&engine, "worker_a", b"synthetic-jit", &engine).await?;

    assert_eq!(started.dind_id, fake_container_id(1));
    assert_eq!(started.runner_id, fake_container_id(2));
    assert_eq!(
        engine.events(),
        [
            "sink-volume",
            "volumes",
            "create",
            "sink-dind",
            "start",
            "create",
            "sink-runner",
            "sink-runner-start-legacy",
            "start",
            "jit",
        ]
    );
    Ok(())
}

#[tokio::test]
async fn stop_before_runner_start_never_requests_start_intent() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive(
        &engine,
        "worker_a",
        b"synthetic-jit",
        PairStop::RunnerCreated,
        &engine,
    )
    .await?;

    assert!(partial.runner_id.is_some());
    assert!(!engine.events().contains(&"sink-runner-start-legacy"));
    Ok(())
}

#[tokio::test]
async fn failed_runner_start_retains_pair_for_journal_reconciliation() {
    let engine = Fake::new();
    engine.fail_on_nth("start", 2);

    let failure = start_pair_with_sink(&engine, "worker_a", b"synthetic-jit", &engine)
        .await
        .expect_err("runner start failure must be preserved");

    assert_eq!(failure.error(), crate::HostError::Docker);
    assert_eq!(failure.phase(), PairStartPhase::RunnerStart);
    assert!(failure.side_effect_may_have_succeeded());
    assert_eq!(
        failure.partial().dind_id.as_deref(),
        Some(fake_container_id(1).as_str())
    );
    assert_eq!(
        failure.partial().runner_id.as_deref(),
        Some(fake_container_id(2).as_str())
    );
    assert_eq!(engine.removed(), Vec::<String>::new());
}

#[tokio::test]
async fn runner_id_persistence_failure_keeps_returned_id() {
    let engine = Fake::new();
    engine.fail_on("sink-runner");

    let failure = start_pair_with_sink(&engine, "worker_a", b"synthetic-jit", &engine)
        .await
        .expect_err("failed journal write must preserve the created runner");

    assert_eq!(failure.phase(), PairStartPhase::RunnerIdentity);
    assert_eq!(
        failure.partial().runner_id.as_deref(),
        Some(fake_container_id(2).as_str())
    );
    assert_eq!(engine.removed(), Vec::<String>::new());
}

#[tokio::test]
async fn jit_write_failure_keeps_started_pair_for_diagnostic_reconciliation() {
    let engine = Fake::new();
    engine.fail_on("jit");

    let failure = start_pair_with_sink(&engine, "worker_a", b"synthetic-jit", &engine)
        .await
        .expect_err("JIT write failure must remain uncertain");

    assert_eq!(failure.phase(), PairStartPhase::JitWrite);
    assert!(failure.side_effect_may_have_succeeded());
    assert_eq!(
        failure.partial().runner_id.as_deref(),
        Some(fake_container_id(2).as_str())
    );
    assert_eq!(engine.removed(), Vec::<String>::new());
}

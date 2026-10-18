use super::admission::{JOB_LIMIT, physical_intent};
use super::helpers::{Scratch, binding, created_scope, record_acquired, runner_name};
use crate::error::HostError;
use crate::journal::{AssignmentIdentity, ConnectionJournal, Journal, ReservationState};
use crate::reconcile::LaunchPhase;
use velnor_runner_github::RunnerReference;

struct StartingRunner {
    _scratch: Scratch,
    scope: ConnectionJournal,
    physical: AssignmentIdentity,
    intent_id: i64,
    claim: crate::journal::PhysicalRunnerStartClaim,
}

async fn starting_runner(nonce: char) -> Result<StartingRunner, HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 70).await?;
    let (physical, intent_id) = physical_intent(&scope, nonce).await?;
    let jit = scope
        .claim_runner_jit(intent_id, &runner_name(nonce))
        .await?
        .ok_or(HostError::Journal)?;
    let runner = RunnerReference {
        id: 770,
        name: runner_name(nonce),
        runner_scale_set_id: 501,
    };
    scope.bind_jit_runner(jit, &runner).await?;
    let claim = scope
        .claim_physical_runner_start(intent_id, &format!("w{}", "9".repeat(32)))
        .await?
        .ok_or(HostError::Journal)?;
    Ok(StartingRunner {
        _scratch: scratch,
        scope,
        physical,
        intent_id,
        claim,
    })
}

#[tokio::test]
async fn ready_finalizer_accepts_exact_prebound_container_ids() -> Result<(), HostError> {
    let start = starting_runner('8').await?;
    start
        .scope
        .bind_worker(
            start.intent_id,
            Some("runner-container"),
            Some("dind-container"),
        )
        .await?;

    start
        .scope
        .record_physical_worker_ready(start.claim, "runner-container", "dind-container")
        .await?;

    let intent = start.scope.intent(start.intent_id).await?;
    assert_eq!(intent.docker_id.as_deref(), Some("runner-container"));
    assert_eq!(intent.dind_id.as_deref(), Some("dind-container"));
    assert_eq!(intent.launch_phase, Some(LaunchPhase::WorkerReady));
    let reservation = start
        .scope
        .reservation(&start.physical)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.state, ReservationState::Reserved);
    Ok(())
}

#[tokio::test]
async fn ready_finalizer_rejects_mismatched_prebound_id_and_keeps_charge() -> Result<(), HostError>
{
    let start = starting_runner('a').await?;
    start
        .scope
        .bind_worker(
            start.intent_id,
            Some("runner-container"),
            Some("dind-container"),
        )
        .await?;

    assert!(
        start
            .scope
            .record_physical_worker_ready(start.claim, "different-runner", "dind-container")
            .await
            .is_err()
    );

    let intent = start.scope.intent(start.intent_id).await?;
    assert_eq!(intent.docker_id.as_deref(), Some("runner-container"));
    assert_eq!(intent.dind_id.as_deref(), Some("dind-container"));
    assert_eq!(intent.launch_phase, Some(LaunchPhase::DockerRequested));
    let reservation = start
        .scope
        .reservation(&start.physical)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.state, ReservationState::Reserved);
    Ok(())
}

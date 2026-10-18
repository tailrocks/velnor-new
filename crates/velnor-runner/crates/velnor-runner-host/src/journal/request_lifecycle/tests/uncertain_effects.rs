use super::admission::{JOB_LIMIT, physical_intent};
use super::helpers::{Scratch, binding, created_scope, record_acquired, runner_name};
use crate::error::HostError;
use crate::journal::{Journal, ReservationState};
use crate::reconcile::LaunchPhase;
use velnor_runner_github::RunnerReference;

#[tokio::test]
async fn uncertain_jit_is_not_reclaimed_after_restart() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 81).await?;
    let (physical, intent_id) = physical_intent(&scope, '6').await?;
    let claim = scope
        .claim_runner_jit(intent_id, &runner_name('6'))
        .await?
        .ok_or(HostError::Journal)?;
    scope.mark_runner_jit_uncertain(claim).await?;
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert!(
        scope
            .claim_runner_jit(intent_id, &runner_name('6'))
            .await?
            .is_none()
    );
    assert_eq!(
        scope.intent(intent_id).await?.launch_phase,
        Some(LaunchPhase::JitUncertain)
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}

#[tokio::test]
async fn uncertain_docker_start_cannot_replay_and_keeps_charge() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 82).await?;
    let (physical, intent_id) = physical_intent(&scope, '7').await?;
    let claim = scope
        .claim_runner_jit(intent_id, &runner_name('7'))
        .await?
        .ok_or(HostError::Journal)?;
    let runner = RunnerReference {
        id: 782,
        name: runner_name('7'),
        runner_scale_set_id: 501,
    };
    scope.bind_jit_runner(claim, &runner).await?;
    let start = scope
        .claim_physical_runner_start(intent_id, &format!("w{}", "c".repeat(32)))
        .await?
        .ok_or(HostError::Journal)?;
    scope.mark_physical_runner_start_uncertain(start).await?;
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert!(
        scope
            .claim_physical_runner_start(intent_id, &format!("w{}", "c".repeat(32)))
            .await?
            .is_none()
    );
    assert_eq!(
        scope.intent(intent_id).await?.launch_phase,
        Some(LaunchPhase::DockerUncertain)
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    assert!(!journal.is_drained().await?);
    Ok(())
}

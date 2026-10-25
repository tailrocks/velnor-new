use super::admission::JOB_LIMIT;
use super::helpers::{
    Scratch, binding, created_scope, prepare_initialized_listener_delivery,
    provision_acquired_request_with_initialization, runner_name,
};
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
    let initialized = provision_acquired_request_with_initialization(&scope, 81, '6', '6').await?;
    let physical = initialized.reservation_assignment.clone();
    let intent_id = initialized.intent_id;
    let (generation, digest) = scope.test_active_authority_identity().await?;
    let claim = scope
        .claim_runner_jit_for_request(
            intent_id,
            initialized.reservation_id,
            81,
            &runner_name('6'),
            generation,
            &digest,
        )
        .await?
        .ok_or(HostError::Journal)?;
    scope.mark_runner_jit_uncertain(claim).await?;
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let (generation, digest) = scope.test_active_authority_identity().await?;
    assert!(
        scope
            .claim_runner_jit_for_request(
                intent_id,
                initialized.reservation_id,
                81,
                &runner_name('6'),
                generation,
                &digest
            )
            .await?
            .is_none()
    );
    assert_eq!(
        scope.intent(intent_id).await?.launch_phase,
        Some(LaunchPhase::JitUncertain)
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Uncertain)
    );
    Ok(())
}

#[tokio::test]
async fn uncertain_listener_delivery_cannot_replay_and_keeps_charge() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let initialized = provision_acquired_request_with_initialization(&scope, 82, '7', '7').await?;
    let provisioned = initialized.provisioned.clone();
    let intent_id = provisioned.intent_id;
    let runner = RunnerReference {
        id: 782,
        name: runner_name('7'),
        runner_scale_set_id: 501,
    };
    let delivery =
        prepare_initialized_listener_delivery(&scope, &binding, &initialized, &runner, 82).await?;
    scope.mark_listener_delivery_uncertain(&delivery).await?;
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert!(
        scope
            .claim_listener_delivery(provisioned.reservation_id)
            .await?
            .is_none()
    );
    assert_eq!(
        scope.intent(intent_id).await?.launch_phase,
        Some(LaunchPhase::JobFactsPublished)
    );
    assert_eq!(
        scope
            .reservation(&provisioned.assignment)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Uncertain)
    );
    assert!(!journal.is_drained().await?);
    Ok(())
}

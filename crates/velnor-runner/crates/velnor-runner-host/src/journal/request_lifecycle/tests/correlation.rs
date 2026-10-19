use super::helpers::{
    Scratch, binding, created_scope, event_source, provision_acquired_request_with_initialization,
    provision_physical_intent, receipt, record_acquired, runner_name,
};
use crate::error::HostError;
use crate::journal::{AssignmentKey, CapacityLimits, Journal, ReservationState};

#[path = "correlation/replay.rs"]
mod replay;

#[tokio::test]
async fn requested_acquire_after_restart_cannot_be_claimed_again() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 88 })
        .await?;
    let source = receipt(&scope, 88, 808, 101).await?;
    scope
        .record_source_authorization(&identity, &source)
        .await?;
    assert!(scope.claim_request_acquire(&identity).await?);
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 88 })
        .await?;
    assert_eq!(
        scope.request_acquire_state(&identity).await?,
        Some(crate::journal::RequestAcquireState::Requested)
    );
    assert!(!scope.claim_request_acquire(&identity).await?);
    Ok(())
}

#[tokio::test]
async fn active_legacy_acquire_blocks_new_jit_without_releasing_capacity() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 100).await?;

    let provisioned = provision_physical_intent(&scope, 'c').await?;
    let physical = provisioned.assignment;
    let intent_id = provisioned.intent_id;
    let (generation, digest) = scope.test_active_authority_identity().await?;

    let legacy_id = journal.begin("launch", "legacy-acquired").await?;
    let connection = journal.connection().await?;
    connection
        .execute(
            "UPDATE intents SET scale_set_id = ?1, runner_request_id = ?2, acquire_attempted = 1, acquire_resolved = 1, acquired = 1, launch_phase = 'acquired' WHERE id = ?3",
            (501, 999, legacy_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;

    assert!(
        scope
            .claim_runner_jit_for_request(
                intent_id,
                provisioned.reservation_id,
                100,
                &runner_name('c'),
                generation,
                &digest
            )
            .await?
            .is_none()
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}

#[tokio::test]
async fn every_acquired_request_must_match_the_bound_source_receipt() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let initialized = provision_acquired_request_with_initialization(&scope, 100, 'd', 'd').await?;
    let physical = initialized.reservation_assignment.clone();
    let intent_id = initialized.intent_id;
    let stale_request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 101 })
        .await?;
    let stale_receipt = receipt(&scope, 101, 201, 101).await?;
    scope
        .record_source_authorization(&stale_request, &stale_receipt)
        .await?;
    assert!(scope.claim_request_acquire(&stale_request).await?);
    scope
        .record_request_acquire(
            &stale_request,
            crate::journal::AssignedAcquireResult::Returned(vec![101]),
        )
        .await?;
    let (generation, digest) = scope.test_active_authority_identity().await?;
    journal
        .connection()
        .await?
        .execute(
            "UPDATE connection_request_authorizations SET binding_digest = ?1 WHERE connection_id = ?2 AND scale_set_id = ?3 AND request_id = ?4",
            ("e".repeat(64), scope.connection_id().as_str(), 501, 101),
        )
        .await
        .map_err(|_| HostError::Journal)?;

    assert!(
        scope
            .claim_runner_jit_for_request(
                intent_id,
                initialized.reservation_id,
                100,
                &runner_name('d'),
                generation,
                &digest
            )
            .await?
            .is_none()
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}

#[tokio::test]
async fn claimed_jit_stays_one_shot_and_holds_capacity_after_restart() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let initialized = provision_acquired_request_with_initialization(&scope, 100, 'f', 'f').await?;
    let physical = initialized.reservation_assignment.clone();
    let intent_id = initialized.intent_id;
    let (generation, digest) = scope.test_active_authority_identity().await?;
    assert!(
        scope
            .claim_runner_jit_for_request(
                intent_id,
                initialized.reservation_id,
                100,
                &runner_name('f'),
                generation,
                &digest
            )
            .await?
            .is_some()
    );
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
                100,
                &runner_name('f'),
                generation,
                &digest
            )
            .await?
            .is_none()
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}

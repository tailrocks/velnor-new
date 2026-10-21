use super::helpers::{
    Scratch, binding, created_scope, event_source, finish_initialized_listener_delivery,
    provision_acquired_request_with_initialization, receipt, record_acquired, runner_name,
};
use crate::error::HostError;
use crate::journal::{
    AssignedAcquireResult, AssignmentIdentity, AssignmentKey, CapacityLimits, ConnectionJournal,
    Journal, RequestAcquireState, SourceAuthorizationResult,
};
use velnor_runner_github::RunnerReference;

pub(super) const JOB_LIMIT: CapacityLimits = CapacityLimits {
    jobs: 1,
    cpu_millicores: 2_000,
    memory_bytes: 25_769_803_776,
};

pub(super) async fn physical_intent(
    scope: &ConnectionJournal,
    nonce: char,
) -> Result<(AssignmentIdentity, i64), HostError> {
    let setup = super::helpers::provision_physical_intent(scope, nonce).await?;
    Ok((setup.assignment, setup.intent_id))
}

#[tokio::test]
async fn same_request_id_has_independent_connection_receipts_and_acquire_state()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let first_binding = binding("essential", 101)?;
    let second_binding = binding("chain", 202)?;
    let first = created_scope(&journal, &first_binding, 501).await?;
    let second = created_scope(&journal, &second_binding, 502).await?;
    let first_identity = first
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 33 })
        .await?;
    let second_identity = second
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 33 })
        .await?;
    assert!(!first.claim_request_acquire(&first_identity).await?);
    assert!(!second.claim_request_acquire(&second_identity).await?);
    let first_receipt = receipt(&first, 33, 77, 101).await?;
    let second_receipt = receipt(&second, 33, 77, 202).await?;

    assert_eq!(
        first
            .record_source_authorization(&first_identity, &first_receipt)
            .await?,
        SourceAuthorizationResult::Recorded
    );
    assert_eq!(
        second
            .record_source_authorization(&second_identity, &second_receipt)
            .await?,
        SourceAuthorizationResult::Recorded
    );
    assert!(first.claim_request_acquire(&first_identity).await?);
    assert!(second.claim_request_acquire(&second_identity).await?);
    first
        .record_request_acquire(&first_identity, AssignedAcquireResult::Returned(vec![33]))
        .await?;
    second
        .record_request_acquire(&second_identity, AssignedAcquireResult::Uncertain)
        .await?;
    assert_eq!(
        first.request_acquire_state(&first_identity).await?,
        Some(RequestAcquireState::Acquired)
    );
    assert_eq!(
        second.request_acquire_state(&second_identity).await?,
        Some(RequestAcquireState::Uncertain)
    );
    drop(first);
    drop(second);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let first = journal.connection_scope(&first_binding).await?;
    let second = journal.connection_scope(&second_binding).await?;
    let first_identity = first
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 33 })
        .await?;
    let second_identity = second
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 33 })
        .await?;
    assert_eq!(
        first.request_acquire_state(&first_identity).await?,
        Some(RequestAcquireState::Acquired)
    );
    assert_eq!(
        second.request_acquire_state(&second_identity).await?,
        Some(RequestAcquireState::Uncertain)
    );
    assert!(!second.claim_request_acquire(&second_identity).await?);
    Ok(())
}

#[tokio::test]
async fn acquire_replay_requires_exact_single_id_and_uncertainty_holds_jit() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 44 })
        .await?;
    let authorized = receipt(&scope, 44, 88, 101).await?;
    scope
        .record_source_authorization(&identity, &authorized)
        .await?;
    assert!(scope.claim_request_acquire(&identity).await?);
    scope
        .record_request_acquire(&identity, AssignedAcquireResult::Returned(vec![44, 45]))
        .await?;
    assert_eq!(
        scope.request_acquire_state(&identity).await?,
        Some(RequestAcquireState::Uncertain)
    );
    assert!(!scope.claim_request_acquire(&identity).await?);
    assert_eq!(
        scope.acquired_authorized_requests(10).await?,
        [] as [crate::journal::AssignmentIdentity; 0]
    );
    let (physical, intent_id) = physical_intent(&scope, 'f').await?;
    let reservation_id = scope
        .reservation(&physical)
        .await?
        .ok_or(HostError::Journal)?
        .reservation_id;
    let (generation, digest) = scope.test_active_authority_identity().await?;
    assert!(
        scope
            .claim_runner_jit_for_request(
                intent_id,
                reservation_id,
                44,
                &runner_name('f'),
                generation,
                &digest
            )
            .await?
            .is_none()
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );

    let empty_reply = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 47 })
        .await?;
    let empty_receipt = receipt(&scope, 47, 91, 101).await?;
    scope
        .record_source_authorization(&empty_reply, &empty_receipt)
        .await?;
    assert!(scope.claim_request_acquire(&empty_reply).await?);
    scope
        .record_request_acquire(&empty_reply, AssignedAcquireResult::Returned(Vec::new()))
        .await?;
    assert_eq!(
        scope.request_acquire_state(&empty_reply).await?,
        Some(RequestAcquireState::Uncertain)
    );
    Ok(())
}

#[tokio::test]
async fn uncertain_acquire_survives_restart_and_keeps_physical_charge() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 46 })
        .await?;
    let authorized = receipt(&scope, 46, 90, 101).await?;
    scope
        .record_source_authorization(&request, &authorized)
        .await?;
    assert!(scope.claim_request_acquire(&request).await?);
    scope
        .record_request_acquire(&request, AssignedAcquireResult::Uncertain)
        .await?;
    let (physical, intent_id) = physical_intent(&scope, '2').await?;
    let reservation_id = scope
        .reservation(&physical)
        .await?
        .ok_or(HostError::Journal)?
        .reservation_id;
    let (generation, digest) = scope.test_active_authority_identity().await?;
    assert!(
        scope
            .claim_runner_jit_for_request(
                intent_id,
                reservation_id,
                46,
                &runner_name('2'),
                generation,
                &digest
            )
            .await?
            .is_none()
    );
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 46 })
        .await?;
    assert_eq!(
        scope.request_acquire_state(&request).await?,
        Some(RequestAcquireState::Uncertain)
    );
    assert!(!scope.claim_request_acquire(&request).await?);
    assert!(
        scope
            .claim_runner_jit_for_request(
                intent_id,
                reservation_id,
                46,
                &runner_name('2'),
                generation,
                &digest
            )
            .await?
            .is_none()
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    Ok(())
}

#[tokio::test]
async fn uncertainty_for_one_request_blocks_another_acquired_request() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 54).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 53 })
        .await?;
    let authorized = receipt(&scope, 53, 92, 101).await?;
    scope
        .record_source_authorization(&request, &authorized)
        .await?;
    assert!(scope.claim_request_acquire(&request).await?);
    scope
        .record_request_acquire(&request, AssignedAcquireResult::Uncertain)
        .await?;
    let (physical, intent_id) = physical_intent(&scope, '3').await?;
    let reservation_id = scope
        .reservation(&physical)
        .await?
        .ok_or(HostError::Journal)?
        .reservation_id;
    let (generation, digest) = scope.test_active_authority_identity().await?;

    assert!(
        scope
            .claim_runner_jit_for_request(
                intent_id,
                reservation_id,
                54,
                &runner_name('3'),
                generation,
                &digest
            )
            .await?
            .is_none()
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    Ok(())
}

mod cancellation_cases;

async fn acquire_request_for_cancellation(
    scope: &ConnectionJournal,
    request_id: i64,
) -> Result<AssignmentIdentity, HostError> {
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await?;
    let source_receipt = receipt(scope, request_id, request_id + 100, 101).await?;
    scope
        .record_source_authorization(&identity, &source_receipt)
        .await?;
    assert!(scope.claim_request_acquire(&identity).await?);
    scope
        .record_request_acquire(&identity, AssignedAcquireResult::Returned(vec![request_id]))
        .await?;
    Ok(identity)
}

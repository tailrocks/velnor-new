use super::admission::{JOB_LIMIT, physical_intent};
use super::helpers::{Scratch, binding, created_scope, record_acquired, runner_name};
use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::{
    AssignedAcquireResult, AssignmentKey, CapacityLimits, Journal, ReservationState,
};

#[tokio::test]
async fn duplicate_scale_set_scope_blocks_jit_for_both_connections() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: JOB_LIMIT.cpu_millicores * 2,
            memory_bytes: JOB_LIMIT.memory_bytes * 2,
        })
        .await?;
    let first_binding = binding("essential", 101)?;
    let first = created_scope(&journal, &first_binding, 501).await?;
    record_acquired(&first, 65).await?;
    let (first_physical, first_intent) = physical_intent(&first, '4').await?;

    let second_binding = binding("chain", 202)?;
    let second = journal.connection_scope(&second_binding).await?;
    second
        .bind_scale_set(501, "integration", SetOwnership::Adopted)
        .await?;
    let (second_physical, second_intent) = physical_intent(&second, '5').await?;

    let first_name = runner_name('4');
    let second_name = runner_name('5');
    assert!(
        first
            .claim_runner_jit(first_intent, &first_name)
            .await?
            .is_none()
    );
    assert!(
        second
            .claim_runner_jit(second_intent, &second_name)
            .await?
            .is_none()
    );
    assert_eq!(
        first
            .reservation(&first_physical)
            .await?
            .map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    assert_eq!(
        second
            .reservation(&second_physical)
            .await?
            .map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    Ok(())
}

#[tokio::test]
async fn a_claimed_runner_name_is_host_wide_and_stays_exclusive_after_restart()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: JOB_LIMIT.cpu_millicores * 2,
            memory_bytes: JOB_LIMIT.memory_bytes * 2,
        })
        .await?;
    let first_binding = binding("essential", 101)?;
    let first_scope = created_scope(&journal, &first_binding, 501).await?;
    record_acquired(&first_scope, 65).await?;
    let second_binding = binding("chain", 202)?;
    let second_scope = created_scope(&journal, &second_binding, 502).await?;
    let second_request = second_scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 66 })
        .await?;
    let second_receipt = super::helpers::receipt(&second_scope, 66, 166, 202).await?;
    second_scope
        .record_source_authorization(&second_request, &second_receipt)
        .await?;
    assert!(second_scope.claim_request_acquire(&second_request).await?);
    second_scope
        .record_request_acquire(&second_request, AssignedAcquireResult::Returned(vec![66]))
        .await?;
    let (first_physical, first_intent) = physical_intent(&first_scope, 'a').await?;
    let (second_physical, second_intent) = physical_intent(&second_scope, 'a').await?;
    let requested_name = runner_name('a');
    let (first_claim, second_claim) = tokio::join!(
        first_scope.claim_runner_jit(first_intent, &requested_name),
        second_scope.claim_runner_jit(second_intent, &requested_name),
    );
    match (first_claim?, second_claim?) {
        (Some(claim), None) => first_scope.mark_runner_jit_uncertain(claim).await?,
        (None, Some(claim)) => second_scope.mark_runner_jit_uncertain(claim).await?,
        _ => return Err(HostError::Journal),
    }
    drop(first_scope);
    drop(second_scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let first_scope = journal.connection_scope(&first_binding).await?;
    let second_scope = journal.connection_scope(&second_binding).await?;
    assert!(
        second_scope
            .claim_runner_jit(second_intent, &requested_name)
            .await?
            .is_none()
    );
    assert!(
        first_scope
            .claim_runner_jit(first_intent, &requested_name)
            .await?
            .is_none()
    );
    assert_eq!(
        first_scope
            .reservation(&first_physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    assert_eq!(
        second_scope
            .reservation(&second_physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}

#[tokio::test]
async fn runner_name_must_match_the_physical_intent_nonce() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: JOB_LIMIT.cpu_millicores,
            memory_bytes: JOB_LIMIT.memory_bytes,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 100).await?;
    let (_, intent_id) = physical_intent(&scope, 'a').await?;

    assert!(scope.claim_runner_jit(intent_id, "v100").await?.is_none());
    assert!(
        scope
            .claim_runner_jit(intent_id, &runner_name('a'))
            .await?
            .is_some()
    );
    Ok(())
}

#[tokio::test]
async fn unscoped_live_work_blocks_jit_without_releasing_capacity() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 52).await?;
    let (physical, intent_id) = physical_intent(&scope, 'c').await?;
    let _unscoped = journal.begin("launch", "unassigned-live-work").await?;

    assert!(
        scope
            .claim_runner_jit(intent_id, &runner_name('c'))
            .await?
            .is_none()
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    Ok(())
}

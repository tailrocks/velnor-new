use super::admission::{JOB_LIMIT, physical_intent};
use super::helpers::{
    Scratch, binding, created_scope, provision_physical_intent_with_sample, record_acquired,
    runner_name,
};
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
        .bind_scale_set(501, "integration", crate::connect::SetOwnership::Adopted)
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

struct DuplicateNameWinner {
    binding: crate::config::RepositoryConnection,
    losing_binding: crate::config::RepositoryConnection,
    assignment: crate::journal::AssignmentIdentity,
    intent_id: i64,
}

#[tokio::test]
async fn a_duplicate_worker_name_is_rejected_before_jit_across_connections() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    configure_two_job_capacity(&journal).await?;
    let (first_binding, first, second_binding, second) =
        prepare_duplicate_name_scopes(&journal).await?;
    let winner = claim_one_duplicate_name(&first, &second, &first_binding, &second_binding).await?;
    drop(first);
    drop(second);
    drop(journal);
    assert_reopened_duplicate_name_is_held(&path, winner).await?;
    Ok(())
}

async fn configure_two_job_capacity(journal: &Journal) -> Result<(), HostError> {
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: JOB_LIMIT.cpu_millicores * 2,
            memory_bytes: JOB_LIMIT.memory_bytes * 2,
        })
        .await
}

async fn prepare_duplicate_name_scopes(
    journal: &Journal,
) -> Result<
    (
        crate::config::RepositoryConnection,
        crate::journal::ConnectionJournal,
        crate::config::RepositoryConnection,
        crate::journal::ConnectionJournal,
    ),
    HostError,
> {
    let first_binding = binding("essential", 101)?;
    let first = created_scope(journal, &first_binding, 501).await?;
    record_acquired(&first, 65).await?;
    let second_binding = binding("chain", 202)?;
    let second = created_scope(journal, &second_binding, 502).await?;
    let request = second
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 66 })
        .await?;
    let receipt = super::helpers::receipt(&second, 66, 166, 202).await?;
    second
        .record_source_authorization(&request, &receipt)
        .await?;
    assert!(second.claim_request_acquire(&request).await?);
    second
        .record_request_acquire(&request, AssignedAcquireResult::Returned(vec![66]))
        .await?;
    Ok((first_binding, first, second_binding, second))
}

async fn claim_one_duplicate_name(
    first: &crate::journal::ConnectionJournal,
    second: &crate::journal::ConnectionJournal,
    first_binding: &crate::config::RepositoryConnection,
    second_binding: &crate::config::RepositoryConnection,
) -> Result<DuplicateNameWinner, HostError> {
    let (first_attempt, second_attempt) = tokio::join!(
        provision_physical_intent_with_sample(first, 'a', 'a'),
        provision_physical_intent_with_sample(second, 'a', 'b'),
    );
    let (winner, binding, losing_binding, winner_scope) = match (first_attempt, second_attempt) {
        (Ok(provisioned), Err(_)) => (
            provisioned,
            first_binding.clone(),
            second_binding.clone(),
            first,
        ),
        (Err(_), Ok(provisioned)) => (
            provisioned,
            second_binding.clone(),
            first_binding.clone(),
            second,
        ),
        _ => return Err(HostError::Journal),
    };
    let intent_id = winner.intent_id;
    let jit = winner_scope
        .claim_runner_jit(intent_id, &runner_name('a'))
        .await?
        .ok_or(HostError::Journal)?;
    winner_scope.mark_runner_jit_uncertain(jit).await?;
    Ok(DuplicateNameWinner {
        binding,
        losing_binding,
        assignment: winner.assignment,
        intent_id,
    })
}

async fn assert_reopened_duplicate_name_is_held(
    path: &std::path::Path,
    winner: DuplicateNameWinner,
) -> Result<(), HostError> {
    let journal = Journal::open(path).await?;
    let winning_scope = journal.connection_scope(&winner.binding).await?;
    let losing_scope = journal.connection_scope(&winner.losing_binding).await?;
    assert!(
        winning_scope
            .claim_runner_jit(winner.intent_id, &runner_name('a'))
            .await?
            .is_none()
    );
    assert_eq!(
        winning_scope
            .reservation(&winner.assignment)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Uncertain)
    );
    let losing_assignment = losing_scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "a".repeat(32),
        })
        .await?;
    assert!(
        losing_scope
            .reservation(&losing_assignment)
            .await?
            .is_none()
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

use super::helpers::{
    Scratch, binding, created_scope, event_source, receipt, record_acquired, runner_name,
};
use crate::error::HostError;
use crate::journal::{
    AssignedAcquireResult, AssignmentIdentity, AssignmentKey, CapacityLimits, ConnectionJournal,
    Journal, RequestAcquireState, ReservationRequest, ReservationResult, SourceAuthorizationResult,
};
use crate::launch::StartPermit;
use velnor_runner_github::RunnerReference;

pub(super) const JOB_LIMIT: CapacityLimits = CapacityLimits {
    jobs: 1,
    cpu_millicores: 2_000,
    memory_bytes: 8_589_934_592,
};

pub(super) async fn physical_intent(
    scope: &ConnectionJournal,
    nonce: char,
) -> Result<(AssignmentIdentity, i64), HostError> {
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_string().repeat(32),
        })
        .await?;
    let permit = StartPermit::test_fixture();
    let generation = permit.sample_generation_id().to_owned();
    let root_digest = permit.docker_root_digest().to_owned();
    let result = scope
        .try_reserve(
            ReservationRequest {
                assignment: &assignment,
                engine_id: "test-engine",
                docker_root_digest: &root_digest,
                cpu_millicores: JOB_LIMIT.cpu_millicores,
                memory_bytes: JOB_LIMIT.memory_bytes,
                sample_generation_id: &generation,
            },
            permit,
        )
        .await?;
    if !matches!(result, ReservationResult::Reserved { .. }) {
        return Err(HostError::Journal);
    }
    let (intent_id, fresh) = scope.begin_launch(&assignment).await?;
    if !fresh {
        return Err(HostError::Journal);
    }
    Ok((assignment, intent_id))
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
    assert!(scope.acquired_authorized_requests(10).await?.is_empty());
    let (physical, intent_id) = physical_intent(&scope, 'f').await?;
    assert!(
        scope
            .claim_runner_jit(intent_id, &runner_name('f'))
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
    assert!(
        scope
            .claim_runner_jit(intent_id, &runner_name('2'))
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
            .claim_runner_jit(intent_id, &runner_name('2'))
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

    assert!(
        scope
            .claim_runner_jit(intent_id, &runner_name('3'))
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
async fn an_unacquired_authorized_request_cannot_start_a_runner() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let (physical, intent_id) = physical_intent(&scope, 'a').await?;

    assert!(
        scope
            .claim_runner_jit(intent_id, &runner_name('a'))
            .await?
            .is_none()
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );

    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 45 })
        .await?;
    let authorized = receipt(&scope, 45, 89, 101).await?;
    scope
        .record_source_authorization(&request, &authorized)
        .await?;

    assert!(
        scope
            .claim_runner_jit(intent_id, &runner_name('a'))
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
async fn completion_for_request_b_does_not_release_runner_a_and_cancel_is_request_only()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 61).await?;
    record_acquired(&scope, 62).await?;
    let cancel = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 63 })
        .await?;
    let cancel_receipt = receipt(&scope, 63, 163, 101).await?;
    scope
        .record_source_authorization(&cancel, &cancel_receipt)
        .await?;
    assert!(scope.claim_request_acquire(&cancel).await?);
    scope
        .record_request_acquire(&cancel, AssignedAcquireResult::Returned(vec![63]))
        .await?;
    let (physical, intent_id) = physical_intent(&scope, 'd').await?;
    let claim = scope
        .claim_runner_jit(intent_id, &runner_name('d'))
        .await?
        .ok_or(HostError::Journal)?;
    let runner = RunnerReference {
        id: 761,
        name: runner_name('d'),
        runner_scale_set_id: 501,
    };
    scope.bind_jit_runner(claim, &runner).await?;
    let start = scope
        .claim_physical_runner_start(intent_id, &format!("w{}", "e".repeat(32)))
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .record_physical_worker_ready(start, "runner-a", "dind-a")
        .await?;

    assert_eq!(
        scope
            .record_job_completed(
                &cancel,
                263,
                &event_source(101, 1063),
                Some("cancelled"),
                None,
                Some("opaque"),
            )
            .await?,
        None
    );
    let request_b = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 62 })
        .await?;
    assert_eq!(
        scope
            .record_job_completed(
                &request_b,
                264,
                &event_source(101, 1062),
                Some("success"),
                Some(&runner),
                Some("opaque-job-id"),
            )
            .await?,
        Some(intent_id)
    );
    assert_eq!(
        scope
            .record_job_started(
                &request_b,
                265,
                &event_source(101, 1062),
                &runner,
                Some("opaque-job-id"),
            )
            .await?,
        intent_id
    );
    assert!(scope.intent(intent_id).await?.runner_completed);
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    Ok(())
}

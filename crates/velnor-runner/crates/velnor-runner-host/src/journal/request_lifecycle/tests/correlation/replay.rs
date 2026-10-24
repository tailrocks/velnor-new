use super::super::helpers::{
    Scratch, binding, created_scope, event_source, finish_initialized_listener_delivery,
    provision_acquired_request_with_initialization, receipt, record_acquired, runner_name,
};
use crate::error::HostError;
use crate::journal::{AssignmentKey, CapacityLimits, Journal, ReservationState};
use crate::reconcile::LaunchPhase;
use velnor_runner_github::RunnerReference;

struct CorrelationFixture {
    _scratch: Scratch,
    _journal: Journal,
    scope: crate::journal::ConnectionJournal,
    physical: crate::journal::AssignmentIdentity,
    canceled: crate::journal::AssignmentIdentity,
    intent_id: i64,
    runner: RunnerReference,
}

#[tokio::test]
async fn jit_witness_does_not_bind_the_started_request_and_events_replay_exactly()
-> Result<(), HostError> {
    let fixture = prepare_correlation_fixture().await?;
    let scope = &fixture.scope;
    let intent_id = fixture.intent_id;
    assert_eq!(
        scope.intent(intent_id).await?.launch_phase,
        Some(LaunchPhase::WorkerReady)
    );

    let canceled_intent = scope
        .record_job_completed(
            &fixture.canceled,
            303,
            &event_source(101, 1102),
            Some("cancelled"),
            None,
            Some("opaque-job"),
        )
        .await?;
    assert_eq!(canceled_intent, None);
    assert!(!scope.intent(intent_id).await?.runner_completed);

    assert_started_and_completed_replay(&fixture).await?;
    Ok(())
}

async fn assert_started_and_completed_replay(
    fixture: &CorrelationFixture,
) -> Result<(), HostError> {
    let scope = &fixture.scope;
    let intent_id = fixture.intent_id;
    let runner = &fixture.runner;
    let running = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 101 })
        .await?;
    let completed = scope
        .record_job_completed(
            &running,
            404,
            &event_source(101, 1101),
            Some("success"),
            Some(runner),
            Some("opaque-job-101"),
        )
        .await?;
    assert_eq!(completed, Some(intent_id));
    assert_eq!(
        scope
            .record_job_started(
                &running,
                405,
                &event_source(101, 1101),
                runner,
                Some("opaque-job-101"),
            )
            .await?,
        intent_id
    );
    assert_eq!(
        scope
            .record_job_started(
                &running,
                405,
                &event_source(101, 1101),
                runner,
                Some("opaque-job-101"),
            )
            .await?,
        intent_id
    );
    assert_eq!(
        scope
            .record_job_completed(
                &running,
                404,
                &event_source(101, 1101),
                Some("success"),
                Some(runner),
                Some("opaque-job-101"),
            )
            .await?,
        Some(intent_id)
    );
    assert!(scope.intent(intent_id).await?.runner_completed);
    assert_eq!(
        scope.intent(intent_id).await?.launch_phase,
        Some(LaunchPhase::RunnerCompleted)
    );
    assert_eq!(
        scope
            .reservation(&fixture.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}

async fn prepare_correlation_fixture() -> Result<CorrelationFixture, HostError> {
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
    let canceled_receipt = receipt(&scope, 102, 202, 101).await?;
    let canceled = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 102 })
        .await?;
    scope
        .record_source_authorization(&canceled, &canceled_receipt)
        .await?;
    if !scope.claim_request_acquire(&canceled).await? {
        return Err(HostError::Journal);
    }
    scope
        .record_request_acquire(
            &canceled,
            crate::journal::AssignedAcquireResult::Returned(vec![102]),
        )
        .await?;
    let initialized = provision_acquired_request_with_initialization(&scope, 101, 'a', 'a').await?;
    let provisioned = initialized.provisioned.clone();
    let physical = provisioned.assignment.clone();
    let intent_id = provisioned.intent_id;
    let runner = RunnerReference {
        id: 701,
        name: runner_name('a'),
        runner_scale_set_id: 501,
    };
    finish_initialized_listener_delivery(&scope, &binding, &initialized, &runner, 101).await?;
    Ok(CorrelationFixture {
        _scratch: scratch,
        _journal: journal,
        scope,
        physical,
        canceled,
        intent_id,
        runner,
    })
}

use super::helpers::{
    TEST_ENGINE_ID, binding, created_scope, event_source, receipt, record_acquired,
};
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, Journal, RequestAcquireState, RequestPhysicalReservationResult,
    ReservationRequest, SourceAuthorizationResult, fixture_storage_provisioning_admission,
};
use crate::launch::StartPermit;
use velnor_runner_github::RunnerReference;

mod fixtures;
mod terminal_available_replay;

const JOB_MEMORY: u64 = 8_589_934_592;
const WITNESS_REQUEST_ID: i64 = 100;

#[tokio::test]
async fn orphaned_reservation_link_fails_journal_reopen() -> Result<(), HostError> {
    let fixture = fixtures::prepare_linked_lifecycle(WITNESS_REQUEST_ID, 101, 'f').await?;
    fixtures::reject_orphaned_reservation_link(fixture).await
}

#[tokio::test]
async fn linked_reservation_reopens_for_same_request_through_cleanup_and_replay()
-> Result<(), HostError> {
    exercise_linked_lifecycle(WITNESS_REQUEST_ID, WITNESS_REQUEST_ID, 'a').await
}

#[tokio::test]
async fn linked_reservation_reopens_for_swapped_request_through_cleanup_and_replay()
-> Result<(), HostError> {
    exercise_linked_lifecycle(WITNESS_REQUEST_ID, 101, 'b').await
}

async fn exercise_linked_lifecycle(
    witness_request_id: i64,
    actual_request_id: i64,
    nonce: char,
) -> Result<(), HostError> {
    let fixture =
        fixtures::prepare_linked_lifecycle(witness_request_id, actual_request_id, nonce).await?;
    let fixture = fixtures::reopen(fixture).await?;
    fixtures::check_started_replay(&fixture).await?;
    let fixture = fixtures::reopen(fixture).await?;
    fixtures::check_completed_replay(&fixture).await?;
    let fixture = fixtures::reopen(fixture).await?;
    fixtures::check_cleanup_and_settlement(&fixture).await?;
    let fixture = fixtures::reopen(fixture).await?;
    fixtures::check_final_replay(&fixture).await
}

#[tokio::test]
async fn linked_reservation_reopens_when_completed_precedes_started() -> Result<(), HostError> {
    let fixture = fixtures::prepare_linked_lifecycle(WITNESS_REQUEST_ID, 102, 'c').await?;
    let actual = fixture
        .scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: fixture.actual_request_id,
        })
        .await?;
    record_completed(&fixture, &actual).await?;

    let fixture = fixtures::reopen(fixture).await?;
    replay_link(
        &fixture.scope,
        fixture.witness_request_id,
        fixture.nonce,
        'd',
        fixture.reservation_id,
        fixture.intent_id,
    )
    .await?;
    let actual = fixture
        .scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: fixture.actual_request_id,
        })
        .await?;
    record_completed(&fixture, &actual).await?;
    record_started(&fixture, &actual).await?;
    record_started(&fixture, &actual).await?;

    let fixture = fixtures::reopen(fixture).await?;
    replay_link(
        &fixture.scope,
        fixture.witness_request_id,
        fixture.nonce,
        'e',
        fixture.reservation_id,
        fixture.intent_id,
    )
    .await?;
    let actual = fixture
        .scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: fixture.actual_request_id,
        })
        .await?;
    record_completed(&fixture, &actual).await?;
    record_started(&fixture, &actual).await?;
    fixtures::check_cleanup_and_settlement(&fixture).await?;

    let fixture = fixtures::reopen(fixture).await?;
    fixtures::check_final_replay(&fixture).await
}

async fn assert_runner_request_association(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
    expected_witness_request_id: i64,
    expected_request_id: Option<i64>,
) -> Result<(), HostError> {
    let reservations = scope.physical_runner_reservations().await?;
    let reservation = reservations
        .iter()
        .find(|reservation| reservation.reservation_id == reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(
        reservation.witness_request_id,
        Some(expected_witness_request_id)
    );
    assert_eq!(reservation.runner_request_id, expected_request_id);
    Ok(())
}

async fn record_completed(
    fixture: &fixtures::LinkedLifecycle,
    actual: &crate::journal::AssignmentIdentity,
) -> Result<(), HostError> {
    assert_eq!(
        fixture
            .scope
            .record_job_completed(
                actual,
                902,
                &fixture.source,
                Some("success"),
                Some(&fixture.runner),
                Some(&fixture.job_id),
            )
            .await?,
        Some(fixture.intent_id),
        "The exact Completed replay must keep its original intent."
    );
    assert_runner_request_association(
        &fixture.scope,
        fixture.reservation_id,
        fixture.witness_request_id,
        Some(fixture.actual_request_id),
    )
    .await
}

async fn record_started(
    fixture: &fixtures::LinkedLifecycle,
    actual: &crate::journal::AssignmentIdentity,
) -> Result<(), HostError> {
    assert_eq!(
        fixture
            .scope
            .record_job_started(
                actual,
                901,
                &fixture.source,
                &fixture.runner,
                Some(&fixture.job_id),
            )
            .await?,
        fixture.intent_id,
        "The exact Started replay must keep its original intent."
    );
    assert_runner_request_association(
        &fixture.scope,
        fixture.reservation_id,
        fixture.witness_request_id,
        Some(fixture.actual_request_id),
    )
    .await
}

async fn authorize(
    scope: &crate::journal::ConnectionJournal,
    request_id: i64,
) -> Result<crate::journal::AssignmentIdentity, HostError> {
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await?;
    if scope
        .record_source_authorization(
            &request,
            &receipt(scope, request_id, request_id + 100, 101).await?,
        )
        .await?
        != SourceAuthorizationResult::Recorded
    {
        return Err(HostError::Journal);
    }
    Ok(request)
}

async fn reserve_linked_request(
    scope: &crate::journal::ConnectionJournal,
    witness: &crate::journal::AssignmentIdentity,
    nonce: char,
) -> Result<(crate::journal::AssignmentIdentity, i64, i64, String, String), HostError> {
    let nonce = nonce.to_string().repeat(32);
    let sample_generation = "9".repeat(32);
    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce,
        })
        .await?;
    let permit = StartPermit::test_fixture_with_engine_id(&sample_generation, TEST_ENGINE_ID);
    let root_digest = permit.docker_root_digest().to_owned();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let result = scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &physical,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
            witness,
        )
        .await?;
    let RequestPhysicalReservationResult::AcquireClaimed {
        reservation_id,
        intent_id,
    } = result
    else {
        return Err(HostError::Journal);
    };
    Ok((
        physical,
        reservation_id,
        intent_id,
        root_digest,
        sample_generation_id,
    ))
}

async fn replay_link(
    scope: &crate::journal::ConnectionJournal,
    witness_request_id: i64,
    nonce: char,
    sample_nonce: char,
    expected_reservation_id: i64,
    expected_intent_id: i64,
) -> Result<(), HostError> {
    let physical_nonce = nonce.to_string().repeat(32);
    let generation = sample_nonce.to_string().repeat(32);
    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: physical_nonce,
        })
        .await?;
    let witness = scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: witness_request_id,
        })
        .await?;
    let permit = StartPermit::test_fixture_with_engine_id(&generation, TEST_ENGINE_ID);
    let root_digest = permit.docker_root_digest().to_owned();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let result = scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &physical,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
            &witness,
        )
        .await?;
    assert!(matches!(
        result,
        RequestPhysicalReservationResult::Existing {
            reservation_id,
            intent_id,
            acquire_state: RequestAcquireState::Acquired,
        } if reservation_id == expected_reservation_id && intent_id == expected_intent_id
    ));
    Ok(())
}

async fn reject_wrong_scope_and_runner(
    journal: &Journal,
    scope: &crate::journal::ConnectionJournal,
    actual_request_id: i64,
    runner: &RunnerReference,
) -> Result<(), HostError> {
    let foreign = binding("chain", 202)?;
    let foreign_scope = created_scope(journal, &foreign, 502).await?;
    let wrong_scope_request = foreign_scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: actual_request_id,
        })
        .await?;
    assert!(
        scope
            .record_job_started(
                &wrong_scope_request,
                900,
                &event_source(101, actual_request_id + 1_000),
                runner,
                Some("wrong-scope-job"),
            )
            .await
            .is_err()
    );
    let actual = scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: actual_request_id,
        })
        .await?;
    let wrong_runner = RunnerReference {
        id: runner.id + 1,
        name: runner.name.clone(),
        runner_scale_set_id: runner.runner_scale_set_id,
    };
    assert!(
        scope
            .record_job_started(
                &actual,
                900,
                &event_source(101, actual_request_id + 1_000),
                &wrong_runner,
                Some("wrong-runner-job"),
            )
            .await
            .is_err()
    );
    Ok(())
}

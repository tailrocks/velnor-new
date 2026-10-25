use crate::error::HostError;
use crate::journal::request_lifecycle::tests::{
    helpers::{self, TEST_ENGINE_ID},
    native_started_intake_fixture,
};
use crate::journal::{
    AssignmentKey, Journal, LocalWorkerExitDisposition, LocalWorkerExitKind,
    LocalWorkerHealthCandidate, PhysicalRunnerJitState, ReservationRequest, ReservationResult,
    fixture_storage_provisioning_admission,
};
use crate::launch::StartPermit;

#[tokio::test]
async fn worker_exit_persists_without_releasing_or_completing_remote_work() -> Result<(), HostError>
{
    let fixture = native_started_intake_fixture().await?;
    let scope = fixture.scope.clone();
    let ready_candidate = one_candidate(
        &scope,
        PhysicalRunnerJitState::Ready,
        crate::reconcile::LaunchPhase::WorkerReady,
    )
    .await?;
    fixture.record_started().await?;
    let path = fixture.database_path.clone();
    let candidate = one_candidate(
        &scope,
        PhysicalRunnerJitState::Started,
        crate::reconcile::LaunchPhase::RunnerStarted,
    )
    .await?;
    let session_intent_id = create_session(&scope).await?;

    assert_eq!(
        scope
            .record_local_worker_exit(
                &ready_candidate,
                LocalWorkerExitKind::Exited { exit_code: 23 }
            )
            .await?,
        LocalWorkerExitDisposition::Recorded
    );
    assert_eq!(
        scope
            .record_local_worker_exit(&candidate, LocalWorkerExitKind::Missing)
            .await?,
        LocalWorkerExitDisposition::AlreadyRecorded
    );
    assert_eq!(all_candidates(&scope).await?, Vec::new());
    assert_held_after_observation(&fixture, &scope, session_intent_id).await?;
    assert_competing_reservation_denied(&scope, 'b').await?;
    let exits = scope.unresolved_local_worker_exits().await?;
    assert_eq!(exits.len(), 1);
    assert_eq!(
        exits[0].runner_container_id(),
        candidate.runner_container_id()
    );
    assert_eq!(
        exits[0].kind(),
        LocalWorkerExitKind::Exited { exit_code: 23 }
    );

    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        _scratch,
        journal,
        scope: old_scope,
        ..
    } = fixture;
    drop(old_scope);
    drop(scope);
    drop(journal);
    let reopened = Journal::open(&path).await?;
    let reopened_scope = reopened
        .connection_scope(&helpers::binding("native-started-intake", 101)?)
        .await?;
    assert_eq!(reopened.occupied_reservation_count().await?, 1);
    assert_eq!(reopened_scope.unresolved_local_worker_exits().await?, exits);
    assert!(
        !reopened_scope
            .session_work_drained(session_intent_id)
            .await?
    );
    assert_competing_reservation_denied(&reopened_scope, 'c').await?;
    Ok(())
}

#[tokio::test]
async fn ready_missing_exit_persists_and_keeps_the_first_observation() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let scope = &fixture.scope;
    let candidate = one_candidate(
        scope,
        PhysicalRunnerJitState::Ready,
        crate::reconcile::LaunchPhase::WorkerReady,
    )
    .await?;
    let session_intent_id = create_session(scope).await?;

    assert_eq!(
        scope
            .record_local_worker_exit(&candidate, LocalWorkerExitKind::Missing)
            .await?,
        LocalWorkerExitDisposition::Recorded
    );
    assert_eq!(
        scope
            .record_local_worker_exit(&candidate, LocalWorkerExitKind::Exited { exit_code: 23 },)
            .await?,
        LocalWorkerExitDisposition::AlreadyRecorded
    );
    assert_eq!(all_candidates(scope).await?, Vec::new());
    assert_held_after_observation(&fixture, scope, session_intent_id).await?;
    assert_competing_reservation_denied(scope, 'd').await?;
    let exits = scope.unresolved_local_worker_exits().await?;
    assert_eq!(exits.len(), 1);
    assert_eq!(exits[0].kind(), LocalWorkerExitKind::Missing);
    Ok(())
}

#[tokio::test]
async fn completed_receipt_makes_an_old_health_candidate_stale() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    fixture.record_started().await?;
    let scope = &fixture.scope;
    let candidate = one_candidate(
        scope,
        PhysicalRunnerJitState::Started,
        crate::reconcile::LaunchPhase::RunnerStarted,
    )
    .await?;
    let completed = scope
        .record_job_completed(
            &fixture.assignment,
            fixture.message_id + 1,
            &fixture.source,
            Some("success"),
            Some(&fixture.runner),
            Some("opaque-started-job"),
        )
        .await?;
    assert_eq!(completed, Some(fixture.intent_id));
    assert_eq!(
        scope
            .record_local_worker_exit(&candidate, LocalWorkerExitKind::Exited { exit_code: 23 })
            .await?,
        LocalWorkerExitDisposition::IgnoredStale
    );
    assert_eq!(scope.unresolved_local_worker_exits().await?, Vec::new());
    let reservations = scope.physical_runner_reservations().await?;
    assert_eq!(reservations.len(), 1);
    assert_eq!(
        reservations[0].completed_request_id,
        Some(fixture.request_id)
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 1);
    Ok(())
}

#[tokio::test]
async fn candidate_from_another_connection_cannot_reuse_a_recorded_exit() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    fixture.record_started().await?;
    let candidate = one_candidate(
        &fixture.scope,
        PhysicalRunnerJitState::Started,
        crate::reconcile::LaunchPhase::RunnerStarted,
    )
    .await?;
    assert_eq!(
        fixture
            .scope
            .record_local_worker_exit(&candidate, LocalWorkerExitKind::Missing)
            .await?,
        LocalWorkerExitDisposition::Recorded
    );
    let other_binding = helpers::binding("worker-health-other", 102)?;
    let other_scope = helpers::created_scope(&fixture.journal, &other_binding, 102).await?;
    assert!(
        other_scope
            .record_local_worker_exit(&candidate, LocalWorkerExitKind::Missing)
            .await
            .is_err()
    );
    assert_eq!(
        fixture.scope.unresolved_local_worker_exits().await?.len(),
        1
    );
    assert_eq!(
        fixture.scope.unresolved_local_worker_exits().await?[0].kind(),
        LocalWorkerExitKind::Missing
    );
    assert_eq!(
        other_scope.unresolved_local_worker_exits().await?,
        Vec::new()
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 1);
    Ok(())
}

async fn one_candidate(
    scope: &crate::journal::ConnectionJournal,
    expected_jit: PhysicalRunnerJitState,
    expected_launch_phase: crate::reconcile::LaunchPhase,
) -> Result<LocalWorkerHealthCandidate, HostError> {
    let reservations = scope.physical_runner_reservations().await?;
    assert_eq!(reservations.len(), 1);
    assert!(matches!(
        reservations[0].ownership,
        crate::journal::PhysicalRunnerOwnership::Known(_)
    ));
    assert_eq!(reservations[0].jit_state, expected_jit);
    assert_eq!(reservations[0].launch_phase, Some(expected_launch_phase));
    let candidates = all_candidates(scope).await?;
    assert_eq!(candidates.len(), 1);
    candidates.into_iter().next().ok_or(HostError::Journal)
}

async fn all_candidates(
    scope: &crate::journal::ConnectionJournal,
) -> Result<Vec<LocalWorkerHealthCandidate>, HostError> {
    let mut after_reservation_id = 0;
    let mut candidates = Vec::new();
    loop {
        let (mut page, scanned_through, has_more) = scope
            .local_worker_health_candidate_page_after(after_reservation_id)
            .await?;
        candidates.append(&mut page);
        if !has_more {
            return Ok(candidates);
        }
        after_reservation_id = scanned_through.ok_or(HostError::Journal)?;
    }
}

async fn create_session(scope: &crate::journal::ConnectionJournal) -> Result<i64, HostError> {
    let prepared = scope
        .prepare_session_create("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .await?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    let session = scope
        .record_created_session(&claim, "worker-health-session", 1)
        .await?;
    Ok(session.intent_id())
}

async fn assert_held_after_observation(
    fixture: &crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
    scope: &crate::journal::ConnectionJournal,
    session_intent_id: i64,
) -> Result<(), HostError> {
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 1);
    assert!(!scope.session_work_drained(session_intent_id).await?);
    let reservations = scope.physical_runner_reservations().await?;
    assert_eq!(reservations.len(), 1);
    assert_eq!(reservations[0].completed_request_id, None);
    assert_ne!(
        reservations[0].jit_state,
        crate::journal::PhysicalRunnerJitState::Completed
    );
    Ok(())
}

async fn assert_competing_reservation_denied(
    scope: &crate::journal::ConnectionJournal,
    nonce: char,
) -> Result<(), HostError> {
    let sample = nonce.to_string().repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&sample, TEST_ENGINE_ID);
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let (cpu_millicores, memory_bytes) = permit.charge();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: sample.clone(),
        })
        .await?;
    let result = scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &assignment,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores,
                memory_bytes,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
        )
        .await?;
    assert_eq!(result, ReservationResult::AtCapacity);
    Ok(())
}

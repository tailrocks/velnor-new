use super::helpers::{
    Scratch, binding, created_scope, event_source, receipt, record_acquired, runner_name,
};
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, CapacityLimits, Journal, ReservationRequest, ReservationResult, ReservationState,
};
use crate::launch::StartPermit;
use crate::reconcile::LaunchPhase;
use velnor_runner_github::RunnerReference;

#[tokio::test]
async fn jit_witness_does_not_bind_the_started_request_and_events_replay_exactly()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 100).await?;
    record_acquired(&scope, 101).await?;
    let canceled = receipt(&scope, 102, 202, 101).await?;
    let canceled_identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 102 })
        .await?;
    scope
        .record_source_authorization(&canceled_identity, &canceled)
        .await?;
    assert!(scope.claim_request_acquire(&canceled_identity).await?);
    scope
        .record_request_acquire(
            &canceled_identity,
            crate::journal::AssignedAcquireResult::Returned(vec![102]),
        )
        .await?;

    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "a".repeat(32),
        })
        .await?;
    let permit = StartPermit::test_fixture();
    let generation = permit.sample_generation_id().to_owned();
    let root_digest = permit.docker_root_digest().to_owned();
    let storage = crate::journal::fixture_admission(&permit)?;
    assert!(matches!(
        scope
            .try_reserve_with_storage(
                ReservationRequest {
                    assignment: &assignment,
                    engine_id: "test-engine",
                    docker_root_digest: &root_digest,
                    cpu_millicores: 2_000,
                    memory_bytes: 8_589_934_592,
                    sample_generation_id: &generation,
                },
                permit,
                storage,
            )
            .await?,
        ReservationResult::Reserved { .. }
    ));
    let (intent_id, fresh) = scope.begin_launch(&assignment).await?;
    assert!(fresh);
    let claim = scope
        .claim_runner_jit(intent_id, &runner_name('a'))
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(claim.witness_request()?.request_id(), Some(100));
    let runner = RunnerReference {
        id: 701,
        name: runner_name('a'),
        runner_scale_set_id: 501,
    };
    scope.bind_jit_runner(claim, &runner).await?;
    assert_eq!(
        scope.intent(intent_id).await?.launch_phase,
        Some(LaunchPhase::JitBound)
    );
    let start = scope
        .claim_physical_runner_start(intent_id, &format!("w{}", "b".repeat(32)))
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        scope.intent(intent_id).await?.launch_phase,
        Some(LaunchPhase::DockerRequested)
    );
    scope
        .record_physical_worker_ready(start, "runner-container", "dind-container")
        .await?;

    let canceled_intent = scope
        .record_job_completed(
            &canceled_identity,
            303,
            &event_source(101, 1102),
            Some("cancelled"),
            None,
            Some("opaque-job"),
        )
        .await?;
    assert_eq!(canceled_intent, None);
    assert!(!scope.intent(intent_id).await?.runner_completed);

    let running = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 101 })
        .await?;
    let completed = scope
        .record_job_completed(
            &running,
            404,
            &event_source(101, 1101),
            Some("success"),
            Some(&runner),
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
                &runner,
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
                &runner,
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
                Some(&runner),
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
        scope.reservation(&assignment).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}

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
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 100).await?;

    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "c".repeat(32),
        })
        .await?;
    let permit = StartPermit::test_fixture();
    let generation = permit.sample_generation_id().to_owned();
    let root_digest = permit.docker_root_digest().to_owned();
    let storage = crate::journal::fixture_admission(&permit)?;
    assert!(matches!(
        scope
            .try_reserve_with_storage(
                ReservationRequest {
                    assignment: &physical,
                    engine_id: "test-engine",
                    docker_root_digest: &root_digest,
                    cpu_millicores: 2_000,
                    memory_bytes: 8_589_934_592,
                    sample_generation_id: &generation,
                },
                permit,
                storage,
            )
            .await?,
        ReservationResult::Reserved { .. }
    ));
    let (intent_id, fresh) = scope.begin_launch(&physical).await?;
    assert!(fresh);

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
            .claim_runner_jit(intent_id, &runner_name('c'))
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
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 100).await?;
    record_acquired(&scope, 101).await?;
    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "d".repeat(32),
        })
        .await?;
    let permit = StartPermit::test_fixture();
    let generation = permit.sample_generation_id().to_owned();
    let root_digest = permit.docker_root_digest().to_owned();
    let storage = crate::journal::fixture_admission(&permit)?;
    assert!(matches!(
        scope
            .try_reserve_with_storage(
                ReservationRequest {
                    assignment: &physical,
                    engine_id: "test-engine",
                    docker_root_digest: &root_digest,
                    cpu_millicores: 2_000,
                    memory_bytes: 8_589_934_592,
                    sample_generation_id: &generation,
                },
                permit,
                storage,
            )
            .await?,
        ReservationResult::Reserved { .. }
    ));
    let (intent_id, _) = scope.begin_launch(&physical).await?;
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
            .claim_runner_jit(intent_id, &runner_name('d'))
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
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 100).await?;
    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "f".repeat(32),
        })
        .await?;
    let permit = StartPermit::test_fixture();
    let generation = permit.sample_generation_id().to_owned();
    let root_digest = permit.docker_root_digest().to_owned();
    let storage = crate::journal::fixture_admission(&permit)?;
    assert!(matches!(
        scope
            .try_reserve_with_storage(
                ReservationRequest {
                    assignment: &physical,
                    engine_id: "test-engine",
                    docker_root_digest: &root_digest,
                    cpu_millicores: 2_000,
                    memory_bytes: 8_589_934_592,
                    sample_generation_id: &generation,
                },
                permit,
                storage,
            )
            .await?,
        ReservationResult::Reserved { .. }
    ));
    let (intent_id, fresh) = scope.begin_launch(&physical).await?;
    assert!(fresh);
    assert!(
        scope
            .claim_runner_jit(intent_id, &runner_name('f'))
            .await?
            .is_some()
    );
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert!(
        scope
            .claim_runner_jit(intent_id, &runner_name('f'))
            .await?
            .is_none()
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}

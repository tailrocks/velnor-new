use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use velnor_runner_github::{Poll, parse_poll};

use crate::{
    error::HostError,
    journal::{
        AssignmentKey, Journal, LocalWorkerExitKind, ReservationRequest, ReservationResult,
        fixture_storage_provisioning_admission,
        request_lifecycle::tests::{NativeStartedIntakeFixture, helpers},
    },
    launch::StartPermit,
    launch::{
        SelectedEngineContext,
        bootstrap::{CleanupRuntimeFixture, cleanup_runtime_fixture_for_base},
    },
};

type QuarantinedFixture = (
    CleanupRuntimeFixture,
    crate::host_controller::LocalWorkerHealthSupervisor,
    Vec<crate::journal::LocalWorkerExitObservation>,
    i64,
);

#[test]
fn missing_runner_stays_charged_until_parsed_completion_and_cleanup_proofs() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "worker-health-completion-cleanup",
        || {
            Box::pin(async {
                Box::pin(missing_runner_cleanup_inner())
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn missing_runner_cleanup_inner() -> Result<(), HostError> {
    let (fixture, mut supervisor, exits, session_intent_id) =
        Box::pin(prepare_quarantined_fixture()).await?;
    let binding = helpers::binding("native-started-intake", 101)?;
    let docker = fixture.server.docker();
    let selected = selected_engine(&fixture.scope, &docker).await?;
    crate::host_controller::supervise_local_worker_health(
        &mut supervisor,
        &fixture.journal,
        &docker,
        &selected,
        &[],
        std::slice::from_ref(&binding),
    )
    .await;
    assert_eq!(fixture.scope.unresolved_local_worker_exits().await?, exits);
    complete_and_release(&fixture, &exits, session_intent_id).await
}

async fn prepare_quarantined_fixture() -> Result<QuarantinedFixture, HostError> {
    let (fixture, supervisor, exits, session_intent_id) = observe_quarantined_fixture().await?;
    Box::pin(reopen_quarantined_fixture(
        fixture,
        supervisor,
        exits,
        session_intent_id,
    ))
    .await
}

async fn observe_quarantined_fixture() -> Result<QuarantinedFixture, HostError> {
    let fixture = super::tests::capacity_retry_fixture().await?;
    fixture.base.record_started().await?;
    let session_intent_id = create_session(&fixture.scope).await?;
    let binding = helpers::binding("native-started-intake", 101)?;
    let docker = fixture.server.docker();
    let selected = selected_engine(&fixture.scope, &docker).await?;
    let mut supervisor = crate::host_controller::LocalWorkerHealthSupervisor::default();
    crate::host_controller::supervise_local_worker_health(
        &mut supervisor,
        &fixture.journal,
        &docker,
        &selected,
        std::slice::from_ref(&binding),
        &[],
    )
    .await;
    let exits = fixture.scope.unresolved_local_worker_exits().await?;
    assert_eq!(exits.len(), 1);
    assert_eq!(exits[0].kind(), LocalWorkerExitKind::Missing);
    assert_held(&fixture, &fixture.scope, session_intent_id).await?;
    assert_competing_reservation_denied(&fixture.scope, 'b').await?;
    Ok((fixture, supervisor, exits, session_intent_id))
}

async fn reopen_quarantined_fixture(
    fixture: CleanupRuntimeFixture,
    supervisor: crate::host_controller::LocalWorkerHealthSupervisor,
    exits: Vec<crate::journal::LocalWorkerExitObservation>,
    session_intent_id: i64,
) -> Result<QuarantinedFixture, HostError> {
    let binding = helpers::binding("native-started-intake", 101)?;
    let CleanupRuntimeFixture {
        base,
        runtime,
        server,
        scope,
        journal,
        reservation_id: _,
    } = fixture;
    let NativeStartedIntakeFixture {
        _scratch: scratch,
        journal: base_journal,
        database_path,
        scope: base_scope,
        request_id,
        message_id,
        intent_id,
        reservation_id,
        provisioned,
        assignment,
        reservation_assignment,
        runner,
        source,
        channel,
    } = base;
    drop(runtime);
    drop(server);
    drop(scope);
    drop(journal);
    drop(base_scope);
    drop(base_journal);

    let reopened = Journal::open(&database_path).await?;
    let reopened_scope = reopened.existing_connection_scope(&binding).await?;
    assert_eq!(reopened.occupied_reservation_count().await?, 1);
    assert_eq!(reopened_scope.unresolved_local_worker_exits().await?, exits);
    assert!(
        !reopened_scope
            .session_work_drained(session_intent_id)
            .await?
    );
    assert_competing_reservation_denied(&reopened_scope, 'c').await?;

    let reopened_base = NativeStartedIntakeFixture {
        _scratch: scratch,
        journal: reopened.clone(),
        database_path,
        scope: reopened_scope,
        request_id,
        message_id,
        intent_id,
        reservation_id,
        provisioned,
        assignment,
        reservation_assignment,
        runner,
        source,
        channel,
    };
    let fixture = cleanup_runtime_fixture_for_base(reopened_base).await?;
    Ok((fixture, supervisor, exits, session_intent_id))
}

async fn complete_and_release(
    fixture: &CleanupRuntimeFixture,
    exits: &[crate::journal::LocalWorkerExitObservation],
    session_intent_id: i64,
) -> Result<(), HostError> {
    let (launch, claim, now) = record_matching_completion(fixture).await?;
    run_local_completion_cleanup(fixture, exits, session_intent_id, &launch, claim, now).await
}

async fn record_matching_completion(
    fixture: &CleanupRuntimeFixture,
) -> Result<
    (
        crate::journal::CompletedLaunch,
        crate::journal::CleanupClaim,
        i64,
    ),
    HostError,
> {
    let poll = parsed_completed_poll(&fixture.base)?;
    let decision = crate::launch::turn::test_scoped_intake(
        &fixture.scope,
        fixture.base.runner.runner_scale_set_id,
        &poll,
    )
    .await
    .map_err(|_| HostError::Journal)?;
    assert!(decision.wake_cleanup);
    assert_eq!(
        fixture
            .scope
            .due_completed_launches(i64::MAX, 1)
            .await?
            .len(),
        1
    );
    assert_eq!(
        fixture.scope.unresolved_local_worker_exits().await?.len(),
        1
    );

    fixture
        .scope
        .record_remote_cleanup(fixture.reservation_id)
        .await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
    let now = unix_seconds()?;
    let launch = fixture
        .scope
        .due_completed_launches(now, 1)
        .await?
        .into_iter()
        .next()
        .ok_or(HostError::Journal)?;
    let claim = fixture
        .scope
        .claim_completion_cleanup_at(launch.intent.id, now, 90)
        .await?
        .ok_or(HostError::Journal)?;
    Ok((launch, claim, now))
}

async fn run_local_completion_cleanup(
    fixture: &CleanupRuntimeFixture,
    exits: &[crate::journal::LocalWorkerExitObservation],
    session_intent_id: i64,
    launch: &crate::journal::CompletedLaunch,
    claim: crate::journal::CleanupClaim,
    now: i64,
) -> Result<(), HostError> {
    fixture
        .server
        .set_capacity_values(
            19 * 1024 * 1024 * 1024,
            19 * 1024 * 1024 * 1024,
            40 * 1024 * 1024 * 1024,
        )
        .await;
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 1);
    assert_eq!(fixture.scope.unresolved_local_worker_exits().await?, exits);
    assert!(
        !fixture
            .scope
            .session_work_drained(session_intent_id)
            .await?
    );
    assert_competing_reservation_denied(&fixture.scope, 'e').await?;
    assert!(
        fixture
            .scope
            .record_completion_runner_absent_at(launch.intent.id, claim.generation, now)
            .await?
    );
    let cleanup = super::tests::run_native_cleanup(fixture, launch, claim).await?;
    assert!(cleanup.is_ok(), "native cleanup returned {cleanup:?}");
    assert_eq!(
        fixture.scope.unresolved_local_worker_exits().await?,
        Vec::new()
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 0);
    assert!(
        fixture
            .scope
            .session_work_drained(session_intent_id)
            .await?
    );
    Ok(())
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

async fn assert_held(
    fixture: &CleanupRuntimeFixture,
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
    assert_eq!(scope.due_completed_launches(i64::MAX, 1).await?, Vec::new());
    Ok(())
}

async fn assert_competing_reservation_denied(
    scope: &crate::journal::ConnectionJournal,
    nonce: char,
) -> Result<(), HostError> {
    let sample = nonce.to_string().repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&sample, helpers::TEST_ENGINE_ID);
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let (cpu_millicores, memory_bytes) = permit.charge();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: sample,
        })
        .await?;
    let result = scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &assignment,
                engine_id: helpers::TEST_ENGINE_ID,
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

async fn selected_engine(
    scope: &crate::journal::ConnectionJournal,
    docker: &bollard::Docker,
) -> Result<SelectedEngineContext, HostError> {
    let engine = scope.engine_context().await?.ok_or(HostError::Ownership)?;
    let info = docker.info().await.map_err(|_| HostError::Docker)?;
    SelectedEngineContext::capture(&engine, &info)
}

fn parsed_completed_poll(fixture: &NativeStartedIntakeFixture) -> Result<Poll, HostError> {
    let source = &fixture.source;
    let event = json!({
        "messageType": "JobCompleted",
        "runnerRequestId": fixture.request_id,
        "runnerId": fixture.runner.id,
        "runnerName": fixture.runner.name,
        "result": "Succeeded",
        "jobId": "opaque-started-job",
        "ownerName": source.owner_name,
        "repositoryName": source.repository_name,
        "jobWorkflowRef": source.job_workflow_ref,
        "workflowRunId": source.workflow_run_id,
        "eventName": source.event_name,
    });
    let body = json!([event]).to_string();
    let envelope = json!({
        "messageId": fixture.message_id,
        "messageType": "RunnerScaleSetJobMessages",
        "body": body,
    })
    .to_string();
    parse_poll(200, &envelope).map_err(|_| HostError::Journal)
}

fn unix_seconds() -> Result<i64, HostError> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_secs(),
    )
    .map_err(|_| HostError::Journal)
}

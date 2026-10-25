use crate::{
    error::HostError,
    journal::{
        AssignmentIdentity, ConnectionJournal, RequestPhysicalReservationResult,
        ReservationRequest, fixture_storage_provisioning_admission,
        request_lifecycle::tests::helpers::{TEST_ENGINE_ID, acquired_physical_fixture_request},
    },
    launch::{
        StartPermit, bootstrap::CleanupRuntimeFixture, completion::diagnostics::DiagnosticsRoot,
    },
};

pub(super) fn stage_error(stage: &str, error: HostError) -> HostError {
    eprintln!("{stage}: {error:?}");
    error
}

pub(super) struct LifecycleFixture {
    pub(super) fixture: CleanupRuntimeFixture,
    pub(super) launch: crate::journal::CompletedLaunch,
    pub(super) competing_physical: AssignmentIdentity,
    pub(super) competing_request: AssignmentIdentity,
    pub(super) diagnostics: DiagnosticsRoot,
    pub(super) now: i64,
    runner_id: String,
    dind_id: String,
}

impl LifecycleFixture {
    pub(super) async fn prepare() -> Result<Self, HostError> {
        let fixture = super::super::capacity_retry_fixture().await?;
        fixture.base.record_started().await?;
        let completion = failed_completion_poll(&fixture)?;
        let intake = crate::launch::test_scoped_intake(
            &fixture.scope,
            fixture.base.runner.runner_scale_set_id,
            &completion,
        )
        .await
        .map_err(|_| HostError::Journal)?;
        if !intake.wake_cleanup || !intake.started_request_ids.is_empty() {
            return Err(HostError::Journal);
        }

        let now = unix_seconds()?;
        let mut launches = fixture.scope.due_completed_launches(now, 2).await?;
        if launches.len() != 1 {
            return Err(HostError::Journal);
        }
        let launch = launches.pop().ok_or(HostError::Journal)?;
        if launch.intent.id != fixture.base.intent_id {
            return Err(HostError::Journal);
        }
        // This fixture starts after remote cleanup has been proven; it exercises local recovery.
        fixture
            .scope
            .record_remote_cleanup(fixture.reservation_id)
            .await?;

        let competing_request = acquired_physical_fixture_request(&fixture.scope).await?;
        let competing_physical = fixture
            .scope
            .assignment_identity(crate::journal::AssignmentKey::Remote {
                kind: "runner".to_owned(),
                subject: "c".repeat(32),
            })
            .await?;
        let identity = fixture
            .scope
            .storage_cleanup_identity(fixture.reservation_id)
            .await?
            .ok_or(HostError::Ownership)?;
        let runner_id = identity.runner_container_id().to_owned();
        let dind_id = identity.dind_container_id().to_owned();
        fixture
            .server
            .expose_owned_containers(fixture.scope.connection_id().as_str())
            .await?;
        fixture
            .server
            .set_capacity_values(
                19 * 1024 * 1024 * 1024,
                19 * 1024 * 1024 * 1024,
                40 * 1024 * 1024 * 1024,
            )
            .await;
        fixture.server.fail_container_delete_once(&dind_id).await;
        let diagnostics = super::super::diagnostics_root(&fixture)?;
        Ok(Self {
            fixture,
            launch,
            competing_physical,
            competing_request,
            diagnostics,
            now,
            runner_id,
            dind_id,
        })
    }
}

pub(super) async fn assert_competing_request_at_capacity(
    state: &LifecycleFixture,
    sample: char,
) -> Result<(), HostError> {
    if !matches!(
        try_competing_request(
            &state.fixture.scope,
            &state.competing_physical,
            &state.competing_request,
            sample,
        )
        .await?,
        RequestPhysicalReservationResult::AtCapacity
    ) {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn assert_competing_request_admitted(
    state: &LifecycleFixture,
    sample: char,
) -> Result<(), HostError> {
    if !matches!(
        try_competing_request(
            &state.fixture.scope,
            &state.competing_physical,
            &state.competing_request,
            sample,
        )
        .await?,
        RequestPhysicalReservationResult::AcquireClaimed { .. }
    ) {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn fail_first_cleanup(state: &LifecycleFixture) -> Result<i64, HostError> {
    let first = state
        .fixture
        .scope
        .claim_completion_cleanup_at(state.launch.intent.id, state.now, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let generation = first.generation;
    if !state
        .fixture
        .scope
        .record_completion_runner_absent_at(state.launch.intent.id, first.generation, state.now)
        .await?
    {
        return Err(HostError::Journal);
    }
    let outcome = super::super::run_native_cleanup_with_diagnostics(
        &state.fixture,
        &state.launch,
        first,
        &state.diagnostics,
    )
    .await?;
    if outcome.is_ok() {
        return Err(HostError::Journal);
    }
    assert_runner_log_capture(&state.fixture, &state.launch)?;
    assert_delete_attempts(state, false).await?;
    assert_eq!(state.fixture.journal.occupied_reservation_count().await?, 1);
    let held = state
        .fixture
        .scope
        .native_completion_reservation(&state.launch)
        .await?
        .ok_or(HostError::Journal)?;
    if !held.remote_cleanup_proven() || held.storage_cleanup_proven() {
        return Err(HostError::Ownership);
    }
    assert_competing_request_at_capacity(state, 'e').await?;
    Ok(generation)
}

pub(super) async fn retry_cleanup(
    state: &LifecycleFixture,
    generation: i64,
) -> Result<(), HostError> {
    let retry_at = state.now.checked_add(1).ok_or(HostError::Journal)?;
    if !state
        .fixture
        .scope
        .retry_completion_cleanup_at(state.launch.intent.id, generation, state.now, retry_at)
        .await
        .map_err(|error| stage_error("schedule cleanup retry", error))?
    {
        eprintln!("schedule cleanup retry: claim generation was not current");
        return Err(HostError::Journal);
    }
    let retry = state
        .fixture
        .scope
        .claim_completion_cleanup_at(state.launch.intent.id, retry_at, 90)
        .await
        .map_err(|error| stage_error("claim cleanup retry", error))?
        .ok_or(HostError::Journal)?;
    if !state
        .fixture
        .scope
        .record_completion_runner_absent_at(state.launch.intent.id, retry.generation, retry_at)
        .await
        .map_err(|error| stage_error("record retry runner absence", error))?
    {
        eprintln!("record retry runner absence: claim was not current");
        return Err(HostError::Journal);
    }
    let outcome = super::super::run_native_cleanup_with_diagnostics(
        &state.fixture,
        &state.launch,
        retry,
        &state.diagnostics,
    )
    .await
    .map_err(|error| stage_error("run retry cleanup", error))?;
    if let Err(failure) = outcome {
        eprintln!("run retry cleanup returned failure: {failure:?}");
        return Err(HostError::Journal);
    }
    assert_delete_attempts(state, true).await?;
    assert_eq!(state.fixture.journal.occupied_reservation_count().await?, 0);
    Ok(())
}

fn failed_completion_poll(
    fixture: &CleanupRuntimeFixture,
) -> Result<velnor_runner_github::Poll, HostError> {
    let base = &fixture.base;
    let message_id = base.message_id.checked_add(1).ok_or(HostError::Journal)?;
    let raw_body = serde_json::json!([{
        "messageType": "JobCompleted",
        "runnerRequestId": base.request_id,
        "runnerId": base.runner.id,
        "runnerName": base.runner.name,
        "result": "Failed",
        "jobId": "opaque-started-job",
        "ownerName": base.source.owner_name,
        "repositoryName": base.source.repository_name,
        "jobWorkflowRef": base.source.job_workflow_ref,
        "workflowRunId": base.source.workflow_run_id,
        "eventName": base.source.event_name,
    }])
    .to_string();
    let envelope = serde_json::json!({
        "messageId": message_id,
        "messageType": "RunnerScaleSetJobMessages",
        "body": raw_body,
    })
    .to_string();
    velnor_runner_github::parse_poll(200, &envelope).map_err(|_| HostError::Frame)
}

fn unix_seconds() -> Result<i64, HostError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_secs(),
    )
    .map_err(|_| HostError::Journal)
}

async fn try_competing_request(
    scope: &ConnectionJournal,
    physical: &AssignmentIdentity,
    request: &AssignmentIdentity,
    sample: char,
) -> Result<RequestPhysicalReservationResult, HostError> {
    let generation = sample.to_string().repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&generation, TEST_ENGINE_ID);
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: physical,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: 8_589_934_592,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
            request,
        )
        .await
}

fn assert_runner_log_capture(
    fixture: &CleanupRuntimeFixture,
    launch: &crate::journal::CompletedLaunch,
) -> Result<(), HostError> {
    let state_path = super::super::super::runner_log_state_path(fixture, launch.intent.id)?;
    let state = serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(state_path).map_err(|_| HostError::Path)?,
    )
    .map_err(|_| HostError::Frame)?;
    if state["phase"] != "complete"
        || state["binding"]["reservation_id"] != fixture.reservation_id
        || state["binding"]["intent_id"] != launch.intent.id
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

async fn assert_delete_attempts(state: &LifecycleFixture, settled: bool) -> Result<(), HostError> {
    let attempts = state
        .fixture
        .server
        .container_delete_attempts()
        .await
        .into_iter()
        .filter(|(id, _)| id == &state.runner_id || id == &state.dind_id)
        .collect::<Vec<_>>();
    let expected = if settled {
        vec![
            (state.runner_id.clone(), 204),
            (state.dind_id.clone(), 500),
            (state.dind_id.clone(), 204),
        ]
    } else {
        vec![(state.runner_id.clone(), 204), (state.dind_id.clone(), 500)]
    };
    if attempts != expected {
        return Err(HostError::Ownership);
    }
    Ok(())
}

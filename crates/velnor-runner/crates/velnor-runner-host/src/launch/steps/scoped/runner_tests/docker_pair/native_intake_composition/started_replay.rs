use super::{Fixture, SET_ID};
use std::cell::Cell;

pub(super) async fn record_started_and_replay(
    fixture: &Fixture,
    request_id: i64,
    ready: &crate::launch::scoped_runtime::RunnerReadyBootstrap,
) -> Result<(), String> {
    let started_body = started_body(request_id, ready.runner().id, &ready.runner().name)?;
    let started_poll = started_poll(started_body)?;
    let intent_id = ready.intent_id();
    let acknowledged_started = Cell::new(false);
    let started = crate::launch::turn::test_started_intake_and_ack(
        &fixture.scope,
        SET_ID,
        &started_poll,
        || async {
            let row = fixture
                .scope
                .intent(intent_id)
                .await
                .map_err(|_| crate::scale_set::EnsureError::AdmissionClosed)?;
            acknowledged_started
                .set(row.launch_phase == Some(crate::reconcile::LaunchPhase::RunnerStarted));
            Ok(())
        },
    )
    .await
    .map_err(|error| format!("Started intake/ACK failed: {error:?}"))?;
    assert_eq!(started, [request_id]);
    assert!(acknowledged_started.get());
    replay_started(fixture, intent_id, request_id, &started_poll).await
}

async fn replay_started(
    fixture: &Fixture,
    intent_id: i64,
    request_id: i64,
    started_poll: &velnor_runner_github::Poll,
) -> Result<(), String> {
    let journal_file = fixture.journal_file();
    let reopened = crate::Journal::open(&journal_file)
        .await
        .map_err(|error| format!("Reopening the journal failed: {error:?}"))?;
    let reopened_scope = reopened
        .connection_scope(&fixture.binding)
        .await
        .map_err(|error| error.to_string())?;
    let reservation_id = fixture
        .scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.intent_id == intent_id)
        .map(|row| row.reservation_id)
        .ok_or_else(|| "Reopened reservation is absent before replay.".to_owned())?;
    let before_replay = reopened_scope
        .native_publication_snapshot(reservation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Reopened publication snapshot is absent.".to_owned())?;
    let replay_acknowledged = Cell::new(false);
    let replayed = crate::launch::turn::test_started_intake_and_ack(
        &reopened_scope,
        SET_ID,
        started_poll,
        || async {
            let row = reopened_scope
                .intent(intent_id)
                .await
                .map_err(|_| crate::scale_set::EnsureError::AdmissionClosed)?;
            replay_acknowledged
                .set(row.launch_phase == Some(crate::reconcile::LaunchPhase::RunnerStarted));
            Ok(())
        },
    )
    .await
    .map_err(|error| format!("Replayed Started intake/ACK failed: {error:?}"))?;
    assert_eq!(replayed, [request_id]);
    assert!(replay_acknowledged.get());
    let after_replay = reopened_scope
        .native_publication_snapshot(reservation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Replayed publication snapshot is absent.".to_owned())?;
    assert_eq!(after_replay, before_replay);
    let rows = reopened_scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .iter()
        .find(|row| row.intent_id == intent_id)
        .ok_or_else(|| "Reopened Started reservation is absent.".to_owned())?;
    assert_eq!(row.started_request_id, Some(request_id));
    assert_eq!(row.runner_request_id, Some(request_id));
    assert_eq!(
        row.jit_state,
        crate::journal::PhysicalRunnerJitState::Started
    );
    assert_eq!(
        row.launch_phase,
        Some(crate::reconcile::LaunchPhase::RunnerStarted)
    );
    assert_eq!(row.runner.as_ref().map(|runner| runner.id), Some(9001));
    Ok(())
}

fn started_body(request_id: i64, runner_id: i64, runner_name: &str) -> Result<String, String> {
    serde_json::to_string(&[serde_json::json!({
        "messageType": "JobStarted",
        "runnerRequestId": request_id,
        "runnerId": runner_id,
        "runnerName": runner_name,
        "jobId": "native-intake-composition-job",
        "ownerName": "owner",
        "repositoryName": "repository",
        "jobWorkflowRef": "owner/repository/.github/workflows/build.yml@refs/heads/main",
        "workflowRunId": 900,
        "eventName": "push"
    })])
    .map_err(|error| error.to_string())
}

fn started_poll(body: String) -> Result<velnor_runner_github::Poll, String> {
    let jobs =
        velnor_runner_github::parse_inner_messages(&body).map_err(|error| error.to_string())?;
    Ok(velnor_runner_github::Poll::Batch(
        velnor_runner_github::ParsedBatch {
            message_id: 78,
            raw_body: body,
            statistics: None,
            jobs,
        },
    ))
}

pub(super) fn listener_failure(
    pending: crate::launch::scoped_runtime::PendingScopedExecution,
) -> String {
    use crate::launch::scoped_runtime::PendingScopedExecution;

    match pending {
        PendingScopedExecution::Jit(failure) => {
            format!(
                "Production listener JIT delivery failed: {:?}",
                failure.failure()
            )
        }
        PendingScopedExecution::Channel(_, failure)
        | PendingScopedExecution::Runner(_, _, failure) => {
            format!("Production listener publication failed: {failure}")
        }
        PendingScopedExecution::RequestTimeout(_) => {
            "Production listener request timed out.".to_owned()
        }
    }
}

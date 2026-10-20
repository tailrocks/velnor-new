//! Queue progress checks for completion messages without runner statistics.

use std::cell::Cell;

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll, QueueSession};

use super::test_positive_poll_once_with_response;
use crate::journal::request_lifecycle::tests::{
    NativeStartedIntakeFixture, native_started_intake_fixture,
};
use crate::launch;
use crate::launch::turn::{Acknowledgement, Settlement};
use crate::listen::inert_link_for_offline_test;
use crate::scale_set::EnsureError;

struct SessionReply;

impl velnor_runner_github::Transport for SessionReply {
    fn exchange(
        &mut self,
        request: &velnor_runner_github::SessionRequest,
    ) -> Result<velnor_runner_github::Exchange, velnor_runner_github::TransportFail> {
        assert_eq!(request.method, velnor_runner_github::Method::Post);
        Ok(velnor_runner_github::Exchange {
            status: 200,
            body: br#"{"sessionId":"progress-test","messageQueueUrl":"https://queue.example.invalid/","messageQueueAccessToken":"queue-token"}"#.to_vec(),
        })
    }
}

fn session() -> Result<QueueSession, String> {
    let mut reply = SessionReply;
    velnor_runner_github::create_session(&mut reply, 501, "owner", "admin-token")
        .map_err(|error| error.to_string())
}

async fn bind_session(
    fixture: &NativeStartedIntakeFixture,
) -> Result<(i64, crate::journal::HeaderAllocation), String> {
    fixture
        .journal
        .bootstrap_header_authority()
        .await
        .map_err(|error| error.to_string())?;
    let prepared = fixture
        .scope
        .prepare_session_create("0123456789abcdef0123456789abcdef")
        .await
        .map_err(|error| error.to_string())?;
    let claim = fixture
        .scope
        .claim_session_create(prepared.local_nonce())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The session creation claim is missing.".to_owned())?;
    let bound = fixture
        .scope
        .record_created_session(&claim, "progress-test", 1)
        .await
        .map_err(|error| error.to_string())?;
    Ok((bound.intent_id(), bound.allocation().clone()))
}

fn completion_response(fixture: &NativeStartedIntakeFixture, message_id: i64) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id,
        raw_body: serde_json::json!([{
            "messageType": "JobCompleted",
            "runnerRequestId": fixture.request_id,
            "runnerId": fixture.runner.id,
            "runnerName": fixture.runner.name,
            "result": "Succeeded",
            "jobId": "opaque-started-job"
        }])
        .to_string(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Completed,
            request_id: Some(fixture.request_id),
            runner_id: Some(fixture.runner.id),
            runner_name: Some(fixture.runner.name.clone()),
            result: Some("Succeeded".to_owned()),
            job_id: Some("opaque-started-job".to_owned()),
            labels: Vec::new(),
            fields: Vec::new(),
            source: fixture.source.clone(),
        }],
    })
}

async fn started_fixture() -> Result<NativeStartedIntakeFixture, String> {
    let fixture = native_started_intake_fixture()
        .await
        .map_err(|error| error.to_string())?;
    fixture
        .record_started()
        .await
        .map_err(|error| error.to_string())?;
    Ok(fixture)
}

#[tokio::test]
async fn missing_stats_completion_ack_allows_the_next_poll() -> Result<(), String> {
    let fixture = started_fixture().await?;
    let (intent_id, allocation) = bind_session(&fixture).await?;
    let mut session = session()?;
    let mut link = inert_link_for_offline_test();
    let acknowledgement_calls = Cell::new(0);

    let outcome = launch::test_positive_poll_once_with_response(
        &mut link,
        fixture.runner.runner_scale_set_id,
        &mut session,
        &fixture.scope,
        intent_id,
        &allocation,
        0,
        completion_response(&fixture, 96),
        "https://queue.example.invalid/messages".to_owned(),
        Some("queue-token".to_owned()),
        async |_link, _session, _path, _queue, _polled| {
            acknowledgement_calls.set(acknowledgement_calls.get() + 1);
            Ok(())
        },
        async |_link, _polled, _queue, _path, _queue_token, _admin_token| {
            Err(EnsureError::Uncertain)
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(outcome.acknowledgement, Acknowledgement::Acknowledged);
    assert_eq!(outcome.settlement, Settlement::Settled);
    assert!(outcome.started_request_ids.is_empty());

    let next_poll = test_positive_poll_once_with_response(
        &mut link,
        fixture.runner.runner_scale_set_id,
        &mut session,
        &fixture.scope,
        intent_id,
        &allocation,
        0,
        Poll::Empty,
        "https://queue.example.invalid/messages".to_owned(),
        Some("queue-token".to_owned()),
        async |_link, _session, _path, _queue, _polled| {
            acknowledgement_calls.set(acknowledgement_calls.get() + 1);
            Ok(())
        },
        async |_link, _polled, _queue, _path, _queue_token, _admin_token| {
            Err(EnsureError::Uncertain)
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(next_poll.acknowledgement, Acknowledgement::NotRequired);
    assert_eq!(next_poll.settlement, Settlement::Settled);
    assert_eq!(acknowledgement_calls.get(), 1);
    Ok(())
}

#[tokio::test]
async fn acknowledgement_error_propagates_before_admission() -> Result<(), String> {
    let fixture = started_fixture().await?;
    let (intent_id, allocation) = bind_session(&fixture).await?;
    let mut session = session()?;
    let mut link = inert_link_for_offline_test();
    let acknowledgement_calls = Cell::new(0);
    let admission_calls = Cell::new(0);

    let result = test_positive_poll_once_with_response(
        &mut link,
        fixture.runner.runner_scale_set_id,
        &mut session,
        &fixture.scope,
        intent_id,
        &allocation,
        0,
        completion_response(&fixture, 98),
        "https://queue.example.invalid/messages".to_owned(),
        Some("queue-token".to_owned()),
        async |_link, _session, _path, _queue, _polled| {
            acknowledgement_calls.set(acknowledgement_calls.get() + 1);
            Err(EnsureError::Uncertain)
        },
        async |_link, _polled, _queue, _path, _queue_token, _admin_token| {
            admission_calls.set(admission_calls.get() + 1);
            Err(EnsureError::Uncertain)
        },
    )
    .await;

    assert!(matches!(result, Err(EnsureError::Uncertain)));
    assert_eq!(acknowledgement_calls.get(), 1);
    assert_eq!(admission_calls.get(), 0);
    Ok(())
}

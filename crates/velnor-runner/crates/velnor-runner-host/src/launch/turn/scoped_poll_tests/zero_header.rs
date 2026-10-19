use std::cell::Cell;

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll, SourceMetadata};

use crate::scale_set::EnsureError;

use super::support::*;
use crate::launch::turn::completion_intake::{IntakeDecision, intake_and_ack_if_only_scoped};
use crate::launch::turn::finish_scoped_poll_response;

#[tokio::test]
async fn handled_zero_header_response_settles_exact_poll_attempt() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(attempt.header_value(), 0);

    let wake = finish_scoped_poll_response(
        &scope,
        intent_id,
        10_101,
        "session-scoped-poll",
        &allocation,
        &Poll::Empty,
        attempt,
        async {
            Ok(IntakeDecision {
                completion_only: true,
                wake_cleanup: true,
                session_delete_candidate: true,
                started_request_ids: Vec::new(),
            })
        },
        0,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(wake.wake_cleanup);
    assert!(wake.session_delete_candidate);

    let replay = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(replay.header_value(), 0);
    Ok(())
}

#[tokio::test]
async fn filtered_empty_keeps_deferred_cursor_and_blocks_session_delete() -> Result<(), String> {
    use crate::launch::CursorDisposition;

    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let outcome = finish_scoped_poll_response(
        &scope,
        intent_id,
        10_101,
        "session-scoped-poll",
        &allocation,
        &Poll::Empty,
        attempt,
        async {
            Ok(IntakeDecision {
                completion_only: false,
                wake_cleanup: false,
                session_delete_candidate: true,
                started_request_ids: Vec::new(),
            })
        },
        72,
    )
    .await
    .map_err(|error| error.to_string())?;

    assert_eq!(outcome.message_id, None);
    assert_eq!(outcome.cursor_disposition, CursorDisposition::Unchanged);
    assert!(!outcome.session_delete_candidate);
    Ok(())
}

#[tokio::test]
async fn failed_response_handling_holds_the_header_poll() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;

    let result = finish_scoped_poll_response(
        &scope,
        intent_id,
        10_101,
        "session-scoped-poll",
        &allocation,
        &Poll::Empty,
        attempt,
        async { Err(EnsureError::AdmissionClosed) },
        0,
    )
    .await;
    assert!(matches!(result, Err(EnsureError::AdmissionClosed)));
    assert!(matches!(
        scope.begin_session_poll(intent_id, &allocation).await,
        Err(_)
    ));
    Ok(())
}

#[tokio::test]
async fn failed_queue_ack_after_inbox_write_holds_the_header_attempt() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let polled = Poll::Batch(ParsedBatch {
        message_id: 77,
        raw_body: r#"[{"messageType":"JobStarted","runnerRequestId":0}]"#.to_owned(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Started,
            request_id: None,
            runner_id: None,
            runner_name: None,
            result: None,
            job_id: None,
            labels: Vec::new(),
            fields: Vec::new(),
            source: SourceMetadata::default(),
        }],
    });
    let ack_attempted = Cell::new(false);
    let inbox_stored_at_ack = Cell::new(false);
    let handling = intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        let pending = scope
            .pending_completion_inbox(4_102_444_800, 10)
            .await
            .map_err(|_| EnsureError::AdmissionClosed)?;
        inbox_stored_at_ack.set(pending.iter().any(|entry| entry.message_id == 77));
        ack_attempted.set(true);
        Err(EnsureError::AdmissionClosed)
    });

    assert!(matches!(
        finish_scoped_poll_response(
            &scope,
            intent_id,
            10_101,
            "session-scoped-poll",
            &allocation,
            &polled,
            attempt,
            handling,
            0,
        )
        .await,
        Err(EnsureError::AdmissionClosed)
    ));
    assert!(ack_attempted.get());
    assert!(inbox_stored_at_ack.get());
    assert!(
        scope
            .pending_completion_inbox(4_102_444_800, 10)
            .await
            .map_err(|error| error.to_string())?
            .iter()
            .any(|entry| entry.message_id == 77)
    );
    assert!(
        scope
            .begin_zero_header_poll(intent_id, &allocation)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn available_message_stays_unacknowledged_at_zero_header() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, _, _) = session_scope(&scratch).await?;
    let polled = Poll::Batch(ParsedBatch {
        message_id: 71,
        raw_body: r#"[{"messageType":"JobAvailable","runnerRequestId":88}]"#.to_owned(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Available,
            request_id: Some(88),
            runner_id: None,
            runner_name: None,
            result: None,
            job_id: Some("opaque-job".to_owned()),
            labels: Vec::new(),
            fields: Vec::new(),
            source: Default::default(),
        }],
    });
    let acknowledged = Cell::new(false);

    let decision = intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;

    assert!(!decision.completion_only);
    assert!(!decision.session_delete_candidate);
    assert!(!acknowledged.get());
    Ok(())
}

#[tokio::test]
async fn mixed_available_and_completion_is_stored_without_acknowledgement() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, _, _) = session_scope(&scratch).await?;
    let polled = Poll::Batch(ParsedBatch {
        message_id: 72,
        raw_body: r#"[{"messageType":"JobAvailable","runnerRequestId":89},{"messageType":"JobCompleted","runnerRequestId":0,"runnerId":902,"runnerName":"v0"}]"#.to_owned(),
        statistics: None,
        jobs: vec![
            InnerJob {
                kind: InnerKind::Available,
                request_id: Some(89),
                runner_id: None,
                runner_name: None,
                result: None,
                job_id: Some("opaque-job".to_owned()),
                labels: Vec::new(),
                fields: Vec::new(),
                source: Default::default(),
            },
            InnerJob {
                kind: InnerKind::Completed,
                request_id: Some(0),
                runner_id: Some(902),
                runner_name: Some("v0".to_owned()),
                result: Some("Succeeded".to_owned()),
                job_id: None,
                labels: Vec::new(),
                fields: Vec::new(),
                source: Default::default(),
            },
        ],
    });
    let acknowledged = Cell::new(false);

    let decision = intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;

    assert!(!decision.completion_only);
    assert!(!decision.session_delete_candidate);
    assert!(!acknowledged.get());
    let pending = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message_id, 72);

    // A later replay cannot discard the Available message just because its
    // lifecycle sibling can be recorded.
    let replay_acknowledged = Cell::new(false);
    intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        replay_acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;
    assert!(!replay_acknowledged.get());
    let still_pending = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(still_pending.len(), 1);
    assert_eq!(still_pending[0].raw_body, pending[0].raw_body);
    Ok(())
}

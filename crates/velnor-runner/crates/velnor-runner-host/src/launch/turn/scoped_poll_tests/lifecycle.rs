use std::cell::Cell;

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll, SourceMetadata};

use crate::scale_set::EnsureError;

use super::support::*;
use crate::launch::turn::completion_intake::{IntakeDecision, intake_and_ack_if_only_scoped};
use crate::launch::turn::finish_scoped_poll_response;

fn started_only_poll() -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: 73,
        raw_body:
            r#"[{"messageType":"JobStarted","runnerRequestId":0,"runnerId":902,"runnerName":"v0"}]"#
                .to_owned(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Started,
            request_id: None,
            runner_id: Some(902),
            runner_name: Some("v0".to_owned()),
            result: None,
            job_id: Some("opaque-job".to_owned()),
            labels: Vec::new(),
            fields: Vec::new(),
            source: SourceMetadata::default(),
        }],
    })
}

#[tokio::test]
async fn started_only_event_is_stored_before_ack_and_replay_is_idempotent() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, session_intent, allocation) = session_scope(&scratch).await?;
    let polled = started_only_poll();
    let acknowledged = Cell::new(false);
    let inbox_stored_before_ack = Cell::new(false);

    let decision = intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        let pending = scope
            .pending_completion_inbox(4_102_444_800, 10)
            .await
            .map_err(|_| EnsureError::AdmissionClosed)?;
        inbox_stored_before_ack.set(pending.iter().any(|entry| entry.message_id == 73));
        acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;

    assert!(decision.completion_only);
    assert!(!decision.session_delete_candidate);
    assert_eq!(decision.started_request_ids, [] as [i64; 0]);
    assert!(acknowledged.get());
    assert!(inbox_stored_before_ack.get());
    let pending = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message_id, 73);
    assert!(
        !scope
            .session_work_drained(session_intent)
            .await
            .map_err(|error| error.to_string())?
    );

    let replay_acknowledged = Cell::new(false);
    intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        replay_acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;
    assert!(replay_acknowledged.get());
    let attempt = scope
        .begin_zero_header_poll(session_intent, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let outcome =
        finish_scoped_poll_response(crate::launch::turn::scoped::ScopedPollFinishContext {
            scoped: &scope,
            session_intent_id: session_intent,
            set_id: 10_101,
            session_id: "session-scoped-poll",
            allocation: &allocation,
            polled: &polled,
            attempt,
            handled: async { Ok((decision, true)) },
            cursor: 0,
        })
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        outcome.cursor_disposition,
        crate::launch::CursorDisposition::ResetToHead
    );
    assert!(!outcome.session_delete_candidate);
    let replayed = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].raw_body, pending[0].raw_body);
    Ok(())
}

#[tokio::test]
async fn partially_recorded_lifecycle_batch_is_acked_only_after_exact_inbox_write()
-> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, session_intent, _) = session_scope(&scratch).await?;
    let request_id = 88;
    let identity = record_acquired_request(&scope, request_id, 74).await?;

    let polled = Poll::Batch(ParsedBatch {
        message_id: 74,
        raw_body: r#"[{"messageType":"JobCompleted","runnerRequestId":88,"result":"cancelled"},{"messageType":"JobStarted","runnerRequestId":999,"runnerId":903,"runnerName":"v1"}]"#.to_owned(),
        statistics: None,
        jobs: vec![
            InnerJob {
                kind: InnerKind::Completed,
                request_id: Some(request_id),
                runner_id: None,
                runner_name: None,
                result: Some("cancelled".to_owned()),
                job_id: None,
                labels: Vec::new(),
                fields: Vec::new(),
                source: SourceMetadata::default(),
            },
            InnerJob {
                kind: InnerKind::Started,
                request_id: Some(999),
                runner_id: Some(903),
                runner_name: Some("v1".to_owned()),
                result: None,
                job_id: None,
                labels: Vec::new(),
                fields: Vec::new(),
                source: SourceMetadata::default(),
            },
        ],
    });
    let acknowledged = Cell::new(false);
    let inbox_stored_before_ack = Cell::new(false);

    let decision = intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        let pending = scope
            .pending_completion_inbox(4_102_444_800, 10)
            .await
            .map_err(|_| EnsureError::AdmissionClosed)?;
        inbox_stored_before_ack.set(pending.iter().any(|entry| entry.message_id == 74));
        acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;

    assert!(decision.completion_only);
    assert_eq!(decision.started_request_ids, [] as [i64; 0]);
    assert!(acknowledged.get());
    assert!(inbox_stored_before_ack.get());
    let pending = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message_id, 74);
    assert!(
        !scope
            .session_work_drained(session_intent)
            .await
            .map_err(|error| error.to_string())?
    );
    assert!(
        scope
            .record_job_completed(
                &identity,
                74,
                &SourceMetadata::default(),
                Some("succeeded"),
                None,
                None,
            )
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn empty_poll_is_delete_candidate_but_has_no_event_ack() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let outcome =
        finish_scoped_poll_response(crate::launch::turn::scoped::ScopedPollFinishContext {
            scoped: &scope,
            session_intent_id: intent_id,
            set_id: 10_101,
            session_id: "session-scoped-poll",
            allocation: &allocation,
            polled: &Poll::Empty,
            attempt,
            handled: async {
                Ok((
                    IntakeDecision {
                        completion_only: false,
                        wake_cleanup: false,
                        session_delete_candidate: true,
                        started_request_ids: Vec::new(),
                    },
                    false,
                ))
            },
            cursor: 0,
        })
        .await
        .map_err(|error| error.to_string())?;
    assert!(!outcome.wake_cleanup);
    assert!(outcome.session_delete_candidate);
    assert!(
        scope
            .session_work_drained(intent_id)
            .await
            .map_err(|error| error.to_string())?
    );
    Ok(())
}

#[tokio::test]
async fn quarantined_body_is_saved_but_not_acknowledged_or_deleted() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, session_intent, _) = session_scope(&scratch).await?;
    let polled = Poll::Quarantined(velnor_runner_github::QuarantinedBatch {
        message_id: 76,
        raw_body: r#"{"malformed":"inner message body"}"#.to_owned(),
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
    assert_eq!(pending[0].message_id, 76);
    assert_eq!(pending[0].raw_body, r#"{"malformed":"inner message body"}"#);
    assert!(
        !scope
            .session_work_drained(session_intent)
            .await
            .map_err(|error| error.to_string())?
    );
    Ok(())
}

#[tokio::test]
async fn reused_message_id_with_changed_body_is_not_acknowledged() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, _, _) = session_scope(&scratch).await?;
    let original = Poll::Batch(ParsedBatch {
        message_id: 75,
        raw_body:
            r#"[{"messageType":"JobStarted","runnerRequestId":0,"runnerId":904,"runnerName":"v2"}]"#
                .to_owned(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Started,
            request_id: None,
            runner_id: Some(904),
            runner_name: Some("v2".to_owned()),
            result: None,
            job_id: None,
            labels: Vec::new(),
            fields: Vec::new(),
            source: SourceMetadata::default(),
        }],
    });
    intake_and_ack_if_only_scoped(&scope, 10_101, &original, || async {
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;

    let mut changed = original.clone();
    let Poll::Batch(batch) = &mut changed else {
        return Err("expected batch".to_owned());
    };
    batch.raw_body =
        r#"[{"messageType":"JobStarted","runnerRequestId":0,"runnerId":905,"runnerName":"v3"}]"#
            .to_owned();
    batch.jobs[0].runner_id = Some(905);
    batch.jobs[0].runner_name = Some("v3".to_owned());
    let acknowledged = Cell::new(false);
    assert!(
        intake_and_ack_if_only_scoped(&scope, 10_101, &changed, || async {
            acknowledged.set(true);
            Ok::<(), EnsureError>(())
        })
        .await
        .is_err()
    );
    assert!(!acknowledged.get());
    Ok(())
}

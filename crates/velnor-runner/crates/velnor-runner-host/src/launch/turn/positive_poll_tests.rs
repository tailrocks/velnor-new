use std::cell::Cell;

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll};

use super::{
    Acknowledgement, CursorDisposition, IntakeDecision, PositiveAdmission, Settlement,
    acknowledgement_after_intake, admit_available, admit_when_ready, authorized_deferred_matches,
    contains_available, cursor_disposition, make_outcome, settle_known_response,
};

fn available_poll() -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: 17,
        raw_body: r#"[{"messageType":"JobAvailable","runnerRequestId":91}]"#.to_owned(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Available,
            request_id: Some(91),
            runner_id: None,
            runner_name: None,
            result: None,
            job_id: Some("job-91".to_owned()),
            labels: Vec::new(),
            fields: Vec::new(),
            source: Default::default(),
        }],
    })
}

#[tokio::test]
async fn available_poll_calls_admission_once_and_no_acquire_stays_unacknowledged()
-> Result<(), String> {
    let polled = available_poll();
    let calls = Cell::new(0);
    let admission = admit_available(&polled, || async {
        calls.set(calls.get() + 1);
        Ok(PositiveAdmission::NoAcquire)
    })
    .await
    .map_err(|error| error.to_string())?;

    assert_eq!(calls.get(), 1);
    assert!(matches!(admission, Some(PositiveAdmission::NoAcquire)));
    assert_eq!(
        acknowledgement_after_intake(
            &IntakeDecision {
                completion_only: false,
                wake_cleanup: false,
                session_delete_candidate: false,
                started_request_ids: Vec::new(),
            },
            &polled
        ),
        Acknowledgement::Deferred
    );
    Ok(())
}

#[tokio::test]
async fn closed_admission_keeps_available_unacked_and_records_lifecycle() -> Result<(), String> {
    use crate::journal::request_lifecycle::tests::native_started_intake_fixture;
    use crate::launch::turn::completion_intake::intake_and_ack_if_only_scoped;

    let fixture = native_started_intake_fixture()
        .await
        .map_err(|error| error.to_string())?;
    fixture
        .record_started()
        .await
        .map_err(|error| error.to_string())?;
    let polled = Poll::Batch(ParsedBatch {
        message_id: 1_502,
        raw_body: r#"[{"messageType":"JobAvailable","runnerRequestId":202},{"messageType":"JobCompleted","runnerRequestId":101,"result":"Succeeded"}]"#.to_owned(),
        statistics: None,
        jobs: vec![
            InnerJob {
                kind: InnerKind::Available,
                request_id: Some(202),
                runner_id: None,
                runner_name: None,
                result: None,
                job_id: Some("job-202".to_owned()),
                labels: Vec::new(),
                fields: Vec::new(),
                source: Default::default(),
            },
            InnerJob {
                kind: InnerKind::Completed,
                request_id: Some(fixture.request_id),
                runner_id: Some(fixture.runner.id),
                runner_name: Some(fixture.runner.name.clone()),
                result: Some("Succeeded".to_owned()),
                job_id: Some("opaque-started-job".to_owned()),
                labels: Vec::new(),
                fields: Vec::new(),
                source: fixture.source.clone(),
            },
        ],
    });
    let acknowledge_called = Cell::new(false);
    let decision = intake_and_ack_if_only_scoped(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        &polled,
        || async {
            acknowledge_called.set(true);
            Ok::<(), crate::scale_set::EnsureError>(())
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    let admission_calls = Cell::new(0);
    let admission = admit_available(&polled, || async {
        admission_calls.set(admission_calls.get() + 1);
        Ok(PositiveAdmission::NoAcquire)
    })
    .await
    .map_err(|error| error.to_string())?;

    assert_eq!(admission_calls.get(), 1);
    assert!(matches!(admission, Some(PositiveAdmission::NoAcquire)));
    assert!(!decision.completion_only);
    assert!(decision.wake_cleanup);
    assert!(!decision.session_delete_candidate);
    assert!(!acknowledge_called.get());
    assert_eq!(
        decision.started_request_ids,
        Vec::<i64>::new(),
        "the fixture's Started event was already durable before this mixed response"
    );
    let outcome = make_outcome(
        decision,
        admission,
        Acknowledgement::Deferred,
        Settlement::Settled,
        &polled,
    );
    assert_eq!(outcome.message_id, Some(1_502));
    assert_eq!(
        outcome.cursor_disposition,
        CursorDisposition::Deferred { message_id: 1_502 }
    );
    let reservation = fixture
        .scope
        .reservation(&fixture.reservation_assignment)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "physical reservation was not retained".to_owned())?;
    assert_eq!(
        reservation.state,
        crate::journal::ReservationState::Reserved
    );
    assert_eq!(
        fixture
            ._journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        1
    );
    assert_eq!(
        fixture
            .scope
            .pending_completion_inbox(4_102_444_800, 10)
            .await
            .map_err(|error| error.to_string())?
            .iter()
            .map(|entry| entry.message_id)
            .collect::<Vec<_>>(),
        [1_502]
    );
    Ok(())
}

#[tokio::test]
async fn admission_gate_blocks_acquire_until_readiness_is_true() -> Result<(), String> {
    let acquire_calls = Cell::new(0);
    let blocked = admit_when_ready(false, || async {
        acquire_calls.set(acquire_calls.get() + 1);
        Ok(PositiveAdmission::NoAcquire)
    })
    .await
    .map_err(|error| error.to_string())?;
    assert!(matches!(blocked, PositiveAdmission::NoAcquire));
    assert_eq!(acquire_calls.get(), 0);

    let ready = admit_when_ready(true, || async {
        acquire_calls.set(acquire_calls.get() + 1);
        Ok(PositiveAdmission::NoAcquire)
    })
    .await
    .map_err(|error| error.to_string())?;
    assert!(matches!(ready, PositiveAdmission::NoAcquire));
    assert_eq!(acquire_calls.get(), 1);
    Ok(())
}

#[tokio::test]
async fn failed_ack_settles_known_get_and_retains_durable_offer() -> Result<(), String> {
    use crate::config::SourcePolicyMode;
    use crate::journal::{AssignmentKey, SourceAuthorizationResult};
    use crate::launch::{
        drive::DeferredAuthorizedOffer,
        turn::scoped_poll_tests::support::{
            Scratch, authorized_source, repository_connection, session_scope,
        },
    };

    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let request_id = 91;
    let message_id = 17;
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|error| error.to_string())?;
    let binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let receipt = crate::launch::steps::source::test_receipt(
        &binding,
        request_id,
        message_id,
        authorized_source(),
        SourcePolicyMode::Repository,
    );
    assert_eq!(
        scope
            .record_source_authorization(&identity, &receipt)
            .await
            .map_err(|error| error.to_string())?,
        SourceAuthorizationResult::Recorded
    );
    let polled = available_poll();
    let connection = repository_connection()?;
    let durable_offer = DeferredAuthorizedOffer::from_poll_receipt(
        &connection,
        &scope,
        10_101,
        &polled,
        request_id,
        receipt,
    )
    .await
    .map_err(|error| format!("{error:?}"))?;
    assert!(authorized_deferred_matches(&durable_offer, &polled, &scope, 10_101).await);

    let attempt = scope
        .begin_session_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let settlement = settle_known_response(
        attempt,
        &allocation,
        &scope,
        10_101,
        "session-scoped-poll",
        intent_id,
    )
    .await;
    let outcome = make_outcome(
        intake_decision(),
        Some(PositiveAdmission::Deferred(durable_offer)),
        Acknowledgement::Uncertain,
        settlement,
        &polled,
    );

    assert_eq!(outcome.acknowledgement, Acknowledgement::Uncertain);
    assert_eq!(outcome.settlement, Settlement::Settled);
    assert_eq!(
        outcome.cursor_disposition,
        CursorDisposition::Deferred { message_id }
    );
    let retained = match outcome.admission {
        Some(PositiveAdmission::Deferred(offer)) => offer,
        _ => return Err("durable authorized offer was lost from the outcome".to_owned()),
    };
    assert_eq!(retained.offer().request_id(), request_id);
    assert_eq!(retained.offer().message_id(), message_id);

    let replayed = scope
        .authorized_offer(request_id, message_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "authorized offer disappeared after ACK uncertainty".to_owned())?;
    assert_eq!(replayed.source_digest(), retained.offer().source_digest());
    assert_eq!(replayed.source_json(), retained.offer().source_json());
    Ok(())
}

#[tokio::test]
async fn native_started_event_is_recorded_before_ack_and_replay_is_idempotent() -> Result<(), String>
{
    use crate::journal::request_lifecycle::tests::native_started_intake_fixture;
    use crate::launch::turn::completion_intake::intake_and_ack_if_only_scoped;
    use crate::reconcile::LaunchPhase;

    let fixture = native_started_intake_fixture()
        .await
        .map_err(|error| error.to_string())?;
    assert_ne!(
        fixture
            .scope
            .intent(fixture.intent_id)
            .await
            .map_err(|error| error.to_string())?
            .launch_phase,
        Some(LaunchPhase::RunnerStarted),
        "the fixture must not record Started before production intake"
    );
    let raw_body = serde_json::to_string(&[serde_json::json!({
        "messageType": "JobStarted",
        "runnerRequestId": fixture.request_id,
        "runnerId": fixture.runner.id,
        "runnerName": fixture.runner.name,
        "jobId": "opaque-started-job",
        "ownerName": fixture.source.owner_name,
        "repositoryName": fixture.source.repository_name,
        "jobWorkflowRef": fixture.source.job_workflow_ref,
        "workflowRunId": fixture.source.workflow_run_id,
        "eventName": fixture.source.event_name,
    })])
    .map_err(|error| error.to_string())?;
    let polled = Poll::Batch(ParsedBatch {
        message_id: fixture.message_id,
        jobs: velnor_runner_github::parse_inner_messages(&raw_body)
            .map_err(|error| error.to_string())?,
        raw_body,
        statistics: None,
    });
    let acknowledged_after_record = Cell::new(false);

    let decision = intake_and_ack_if_only_scoped(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        &polled,
        || async {
            let intent = fixture
                .scope
                .intent(fixture.intent_id)
                .await
                .map_err(|_| crate::scale_set::EnsureError::AdmissionClosed)?;
            acknowledged_after_record.set(intent.launch_phase == Some(LaunchPhase::RunnerStarted));
            Ok::<(), crate::scale_set::EnsureError>(())
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    assert!(decision.completion_only);
    assert_eq!(decision.started_request_ids, [fixture.request_id]);
    assert!(acknowledged_after_record.get());
    assert_eq!(
        fixture
            .scope
            .intent(fixture.intent_id)
            .await
            .map_err(|error| error.to_string())?
            .launch_phase,
        Some(LaunchPhase::RunnerStarted)
    );

    let replay = intake_and_ack_if_only_scoped(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        &polled,
        || async {
            let intent = fixture
                .scope
                .intent(fixture.intent_id)
                .await
                .map_err(|_| crate::scale_set::EnsureError::AdmissionClosed)?;
            assert_eq!(intent.launch_phase, Some(LaunchPhase::RunnerStarted));
            Ok::<(), crate::scale_set::EnsureError>(())
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(replay.started_request_ids, [fixture.request_id]);
    Ok(())
}

#[tokio::test]
async fn empty_poll_does_not_call_admission() -> Result<(), String> {
    let calls = Cell::new(0);
    let polled = Poll::Empty;
    let admission = admit_available(&polled, || async {
        calls.set(calls.get() + 1);
        Ok(PositiveAdmission::NoAcquire)
    })
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(calls.get(), 0);
    assert!(admission.is_none());
    assert_eq!(
        acknowledgement_after_intake(
            &IntakeDecision {
                completion_only: false,
                wake_cleanup: false,
                session_delete_candidate: false,
                started_request_ids: Vec::new(),
            },
            &polled
        ),
        Acknowledgement::NotRequired
    );
    Ok(())
}

#[test]
fn unavailable_response_is_not_an_available_offer() {
    let polled = Poll::Batch(ParsedBatch {
        message_id: 18,
        raw_body: "[]".to_owned(),
        statistics: None,
        jobs: Vec::new(),
    });
    assert!(!contains_available(&polled));
    assert_eq!(
        acknowledgement_after_intake(
            &IntakeDecision {
                completion_only: false,
                wake_cleanup: false,
                session_delete_candidate: false,
                started_request_ids: Vec::new(),
            },
            &polled
        ),
        Acknowledgement::Deferred
    );
}

#[tokio::test]
async fn duplicate_available_rows_trigger_only_one_admission_callback() -> Result<(), String> {
    let mut polled = available_poll();
    let Poll::Batch(batch) = &mut polled else {
        return Err("test poll is not a batch".to_owned());
    };
    batch.jobs.push(batch.jobs[0].clone());
    let calls = Cell::new(0);

    let result = admit_available(&polled, || async {
        calls.set(calls.get() + 1);
        Ok(PositiveAdmission::NoAcquire)
    })
    .await;
    assert!(result.is_ok());
    assert_eq!(calls.get(), 1);
    Ok(())
}

#[test]
fn acquired_ownership_survives_acknowledgement_and_settlement_uncertainty() {
    let token = Box::new(ReservationToken { request_id: 91 });
    let token_address = std::ptr::addr_of!(*token);
    let outcome = make_outcome(
        intake_decision(),
        Some(token),
        Acknowledgement::Uncertain,
        Settlement::Uncertain,
        &available_poll(),
    );

    let retained = outcome.admission.expect("acquired token is retained");
    assert_eq!(retained.request_id, 91);
    assert!(std::ptr::eq(token_address, &*retained));
    assert_eq!(outcome.acknowledgement, Acknowledgement::Uncertain);
    assert_eq!(outcome.settlement, Settlement::Uncertain);
    assert_eq!(outcome.message_id, Some(17));
    assert_eq!(
        outcome.cursor_disposition,
        CursorDisposition::Deferred { message_id: 17 }
    );
}

#[test]
fn settlement_uncertainty_keeps_an_acknowledged_acquired_token() {
    let token = Box::new(ReservationToken { request_id: 92 });
    let token_address = std::ptr::addr_of!(*token);
    let outcome = make_outcome(
        intake_decision(),
        Some(token),
        Acknowledgement::Acknowledged,
        Settlement::Uncertain,
        &available_poll(),
    );

    let retained = outcome.admission.expect("acquired token is retained");
    assert_eq!(retained.request_id, 92);
    assert!(std::ptr::eq(token_address, &*retained));
    assert_eq!(outcome.acknowledgement, Acknowledgement::Acknowledged);
    assert_eq!(outcome.settlement, Settlement::Uncertain);
    assert_eq!(outcome.cursor_disposition, CursorDisposition::ResetToHead);
}

#[test]
fn cursor_retries_known_unacknowledged_head_and_resets_after_ack() {
    assert_eq!(
        cursor_disposition(Some(19), Acknowledgement::Deferred, &available_poll()),
        CursorDisposition::Deferred { message_id: 19 }
    );
    assert_eq!(
        cursor_disposition(Some(19), Acknowledgement::Uncertain, &available_poll()),
        CursorDisposition::Deferred { message_id: 19 }
    );
    assert_eq!(
        cursor_disposition(Some(19), Acknowledgement::Acknowledged, &available_poll()),
        CursorDisposition::ResetToHead
    );
    assert_eq!(
        cursor_disposition(Some(0), Acknowledgement::Deferred, &available_poll()),
        CursorDisposition::Unchanged
    );
    assert_eq!(
        cursor_disposition(Some(0), Acknowledgement::Acknowledged, &available_poll()),
        CursorDisposition::ResetToHead
    );
    assert_eq!(
        cursor_disposition(None, Acknowledgement::NotRequired, &Poll::Empty),
        CursorDisposition::Unchanged
    );
}

struct ReservationToken {
    request_id: i64,
}

fn intake_decision() -> IntakeDecision {
    IntakeDecision {
        completion_only: false,
        wake_cleanup: false,
        session_delete_candidate: false,
        started_request_ids: vec![91],
    }
}

#[path = "positive_poll_tests/assigned_total.rs"]
mod assigned_total;

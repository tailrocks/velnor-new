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
            source: velnor_runner_github::SourceMetadata::default(),
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
async fn terminal_acquire_results_keep_ack_and_cursor_rules() -> Result<(), String> {
    use crate::journal::{AssignmentKey, RequestAcquireState};
    use crate::launch::turn::scoped_poll_tests::support::{Scratch, session_scope};
    use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

    struct SessionReply;

    impl Transport for SessionReply {
        fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
            assert_eq!(request.method, Method::Post);
            Ok(Exchange {
                status: 200,
                body: br#"{"sessionId":"session-scoped-poll","messageQueueUrl":"https://queue.example.invalid/","messageQueueAccessToken":"queue-token"}"#.to_vec(),
            })
        }
    }

    for (state, expected_ack) in [
        (RequestAcquireState::Rejected, Acknowledgement::Deferred),
        (RequestAcquireState::NoEffect, Acknowledgement::Uncertain),
    ] {
        let scratch = Scratch::new()?;
        let (scope, intent_id, allocation) = session_scope(&scratch).await?;
        let mut reply = SessionReply;
        let session =
            velnor_runner_github::create_session(&mut reply, 10_101, "owner", "admin-token")
                .map_err(|error| error.to_string())?;
        let request = scope
            .assignment_identity(AssignmentKey::AssignedRequest { request_id: 91 })
            .await
            .map_err(|error| error.to_string())?;
        let receipt = crate::launch::steps::KnownNoAcquire::test_fixture(request, state, true);
        let attempt = scope
            .begin_session_poll(intent_id, &allocation)
            .await
            .map_err(|error| error.to_string())?;
        let mut link = crate::listen::inert_link_for_offline_test();
        let admin_base = "https://127.0.0.1:1/".to_owned();
        link.set_base(&admin_base)
            .map_err(|error| error.to_string())?;
        let outcome = super::finish_positive_poll(
            &mut link,
            &session,
            &scope,
            intent_id,
            10_101,
            &allocation,
            0,
            attempt,
            available_poll(),
            admin_base,
            None,
            "https://127.0.0.1:1/".to_owned(),
            IntakeDecision {
                completion_only: false,
                wake_cleanup: false,
                session_delete_candidate: false,
                started_request_ids: Vec::new(),
            },
            Some(PositiveAdmission::KnownNoAcquire(receipt)),
        )
        .await
        .map_err(|error| error.to_string())?;
        assert_eq!(outcome.acknowledgement, expected_ack);
        assert_eq!(outcome.settlement, Settlement::Settled);
        assert_eq!(
            outcome.cursor_disposition,
            CursorDisposition::Deferred { message_id: 17 }
        );
    }
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
                source: velnor_runner_github::SourceMetadata::default(),
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
            .journal
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

#[path = "positive_poll_tests/ack_replay.rs"]
mod ack_replay;
#[path = "positive_poll_tests/ack_uncertain.rs"]
mod ack_uncertain;
#[path = "positive_poll_tests/assigned_total.rs"]
mod assigned_total;
#[path = "positive_poll_tests/ownership_cursor.rs"]
mod ownership_cursor;

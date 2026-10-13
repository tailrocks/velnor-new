//! Durable quarantine must precede acknowledgement and leave valid offers progressing.

use velnor_runner_github::{
    Exchange, InnerJob, InnerKind, Method, ParsedBatch, Poll, SessionRequest, Transport,
    TransportFail,
};

use super::admission;
use super::completion_intake::{intake_and_ack_if_only, intake_poll};
use crate::launch::capacity::Admit;
use crate::launch::steps::acknowledge;
use crate::launch::{Drive, Lane};
use crate::launch_harness::open;
use crate::launch_test_support::Engine;
use crate::scale_set::EnsureError;

#[derive(Default)]
struct AckScript {
    requests: Vec<(Method, String)>,
}

impl Transport for AckScript {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.requests.push((request.method, request.path.clone()));
        Ok(Exchange {
            status: 204,
            body: Vec::new(),
        })
    }
}

impl Lane for AckScript {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }

    fn on_queue(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }
}

#[tokio::test]
async fn malformed_event_is_quarantined_then_later_offer_admits() -> Result<(), String> {
    let (_scratch, journal) = open("completion-inbox-malformed-progress").await?;
    let malformed = parse_malformed_event()?;
    let Poll::Quarantined(event) = &malformed else {
        return Err("expected bounded quarantine".to_owned());
    };
    let mut ack = AckScript::default();
    let context = Drive {
        set_id: 7,
        queue_path: "messages".to_owned(),
        queue_token: "queue-token".to_owned(),
        admin_token: "admin-token".to_owned(),
    };
    let ack_batch = ParsedBatch {
        message_id: event.message_id,
        raw_body: event.raw_body.clone(),
        statistics: None,
        jobs: Vec::new(),
    };
    let intake = intake_and_ack_if_only(&journal, 7, &malformed, || async {
        let inbox = journal
            .pending_completion_inbox(0, 4)
            .await
            .map_err(|_| super::completion_intake::completion_error())?;
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].scale_set_id, 7);
        assert_eq!(inbox[0].message_id, event.message_id);
        assert_eq!(inbox[0].raw_body, event.raw_body);
        acknowledge(&mut ack, &context, &ack_batch)
    })
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(ack.requests, [(Method::Delete, "messages/59".to_owned())]);
    assert!(intake.completion_only);
    assert!(!intake.wake_cleanup);

    let valid = parse_available_event()?;
    let previous_requests = ack.requests.len();
    let next = intake_and_ack_if_only(&journal, 7, &valid, || async {
        ack.requests
            .push((Method::Delete, "should-not-ack".to_owned()));
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?;
    assert!(!next.completion_only);
    assert_eq!(ack.requests.len(), previous_requests);
    let decision = admission(&Engine::new(), &journal, 1, 1, 0, &valid)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(decision, Admit::Start { stop: true });
    Ok(())
}

#[tokio::test]
async fn inbox_conflict_prevents_quarantine_ack() -> Result<(), String> {
    let (_scratch, journal) = open("completion-inbox-conflict-no-ack").await?;
    journal
        .store_completion_inbox(7, 59, "[]")
        .await
        .map_err(|error| error.to_string())?;
    let malformed = parse_malformed_event()?;
    let mut ack = AckScript::default();
    let context = Drive {
        set_id: 7,
        queue_path: "messages".to_owned(),
        queue_token: "queue-token".to_owned(),
        admin_token: "admin-token".to_owned(),
    };
    let batch = ParsedBatch {
        message_id: 59,
        raw_body: "[{bad json]".to_owned(),
        statistics: None,
        jobs: Vec::new(),
    };
    let result = intake_and_ack_if_only(&journal, 7, &malformed, || async {
        acknowledge(&mut ack, &context, &batch)
    })
    .await;
    assert!(result.is_err());
    assert!(ack.requests.is_empty());
    Ok(())
}

#[tokio::test]
async fn unmatched_completion_does_not_block_an_available_offer() -> Result<(), String> {
    let (_scratch, journal) = open("completion-inbox-mixed").await?;
    let event = Poll::Batch(ParsedBatch {
        message_id: 58,
        raw_body: r#"[{"messageType":"JobAvailable","runnerRequestId":62},{"messageType":"JobCompleted","runnerRequestId":0,"runnerId":902,"runnerName":"v0"}]"#.to_owned(),
        statistics: None,
        jobs: vec![
            InnerJob {
                kind: InnerKind::Available,
                request_id: Some(62),
                job_id: Some("1001".to_owned()),
                labels: Vec::new(),
                runner_id: None,
                runner_name: None,
                result: None,
                fields: Vec::new(),
            },
            InnerJob {
                kind: InnerKind::Completed,
                request_id: Some(0),
                job_id: None,
                labels: Vec::new(),
                runner_id: Some(902),
                runner_name: Some("v0".to_owned()),
                result: Some("Succeeded".to_owned()),
                fields: Vec::new(),
            },
        ],
    });
    let intake = intake_poll(&journal, 7, &event)
        .await
        .map_err(|error| error.to_string())?;
    assert!(!intake.completion_only && intake.wake_cleanup);
    let decision = admission(&Engine::new(), &journal, 1, 1, 0, &event)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(decision, Admit::Start { stop: true });
    let inbox = journal
        .pending_completion_inbox(0, 4)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(inbox.len(), 1);
    let Poll::Batch(batch) = &event else {
        return Err("expected parsed batch".to_owned());
    };
    assert_eq!(inbox[0].raw_body, batch.raw_body);
    Ok(())
}

fn parse_malformed_event() -> Result<Poll, String> {
    velnor_runner_github::parse_poll(
        200,
        r#"{"messageId":59,"messageType":"RunnerScaleSetJobMessages","body":"[{bad json]"}"#,
    )
    .map_err(|error| error.to_string())
}

fn parse_available_event() -> Result<Poll, String> {
    velnor_runner_github::parse_poll(
        200,
        r#"{"messageId":60,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAvailable\",\"runnerRequestId\":62}]"}"#,
    )
    .map_err(|error| error.to_string())
}

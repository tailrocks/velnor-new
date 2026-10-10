//! Durable quarantine must precede acknowledgement and leave valid offers progressing.

use velnor_runner_github::{
    Exchange, InnerJob, InnerKind, Method, ParsedBatch, Poll, SessionRequest, Transport,
    TransportFail,
};

use super::admission;
use super::completion_intake::{intake_and_ack_if_only, intake_poll};
use crate::Journal;
use crate::launch::capacity::Admit;
use crate::launch::drive_offer;
use crate::launch::steps::acknowledge;
use crate::launch::{Drive, Lane};
use crate::launch_harness::open;
use crate::launch_harness::{Mode, Script};
use crate::launch_test_support::{Engine, valid_worker_volume};
use crate::scale_set::EnsureError;

const EXISTING_BACKLOG_ROWS: i64 = 128;

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

    fn use_github_api(&mut self) -> Result<(), EnsureError> {
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
        docker_engine_id: None,
        owner: String::new(),
        repo: String::new(),
        pat: String::new(),
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
        docker_engine_id: None,
        owner: String::new(),
        repo: String::new(),
        pat: String::new(),
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
    assert_eq!(ack.requests, Vec::new());
    Ok(())
}

#[tokio::test]
async fn unmatched_completion_does_not_block_an_available_offer() -> Result<(), String> {
    let (_scratch, journal) = open("completion-inbox-mixed").await?;
    let event = unmatched_mixed_event(58);
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

#[tokio::test]
async fn overflow_body_survives_restart_and_mixed_offer_starts_once() -> Result<(), String> {
    let (scratch, journal) = open("completion-inbox-full-quarantine").await?;
    fill_inbox(&journal).await?;
    let event = unmatched_mixed_event(EXISTING_BACKLOG_ROWS);
    let context = Drive {
        set_id: 7,
        queue_path: "messages".to_owned(),
        queue_token: "queue-token".to_owned(),
        admin_token: "admin-token".to_owned(),
        docker_engine_id: None,
        owner: String::new(),
        repo: String::new(),
        pat: String::new(),
    };
    let starts = start_overflow_offer(&journal, &event, &context).await?;
    drop(journal);
    let path = scratch.file();
    verify_overflow_replay(&path, &event, &context, starts).await?;
    Ok(())
}

async fn start_overflow_offer(
    journal: &Journal,
    event: &Poll,
    context: &Drive,
) -> Result<usize, String> {
    let first_intake = intake_poll(journal, 7, event)
        .await
        .map_err(|error| error.to_string())?;
    assert!(!first_intake.completion_only && first_intake.wake_cleanup);
    let decision = admission(&Engine::new(), journal, 1, 1, 0, event)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(decision, Admit::Start { stop: true });
    assert!(
        journal
            .store_completion_inbox(7, EXISTING_BACKLOG_ROWS, "replaced")
            .await
            .is_err()
    );
    let mut first = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let starts = std::cell::Cell::new(0);
    let result = drive_offer(&mut first, context, event, journal, |volume, _, _| {
        starts.set(starts.get() + usize::from(valid_worker_volume(volume)));
        async {
            Ok(crate::worker::Started {
                dind_id: "dind-128".to_owned(),
                runner_id: "runner-128".to_owned(),
            })
        }
    })
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(
        result.map(|started| started.runner_id),
        Some("runner-128".to_owned())
    );
    assert_eq!(starts.get(), 1);
    assert_eq!(first.calls, ["acquire", "jit", "ack"]);
    Ok(starts.get())
}

async fn verify_overflow_replay(
    path: &std::path::Path,
    event: &Poll,
    context: &Drive,
    starts: usize,
) -> Result<(), String> {
    let restarted = Journal::open(path)
        .await
        .map_err(|error| error.to_string())?;
    for message_id in 0..EXISTING_BACKLOG_ROWS {
        assert!(
            restarted
                .store_completion_inbox(7, message_id, "replaced")
                .await
                .is_err()
        );
    }
    let Poll::Batch(batch) = event else {
        return Err("expected mixed batch".to_owned());
    };
    assert!(
        restarted
            .store_completion_inbox(7, batch.message_id, "replaced")
            .await
            .is_err()
    );
    restarted
        .store_completion_inbox(7, batch.message_id, &batch.raw_body)
        .await
        .map_err(|error| error.to_string())?;
    let replay_intake = intake_poll(&restarted, 7, event)
        .await
        .map_err(|error| error.to_string())?;
    assert!(!replay_intake.completion_only);
    let mut replay = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let start_count = std::cell::Cell::new(starts);
    let again = drive_offer(&mut replay, context, event, &restarted, |_, _, _| {
        start_count.set(start_count.get() + 1);
        async { Err(crate::HostError::Docker) }
    })
    .await
    .map_err(|error| error.to_string())?;
    assert!(again.is_none());
    assert_eq!(start_count.get(), 1);
    assert_eq!(replay.calls, ["ack"]);
    Ok(())
}

async fn fill_inbox(journal: &Journal) -> Result<(), String> {
    for message_id in 0..EXISTING_BACKLOG_ROWS {
        journal
            .store_completion_inbox(7, message_id, "[]")
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn unmatched_mixed_event(message_id: i64) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id,
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
    })
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

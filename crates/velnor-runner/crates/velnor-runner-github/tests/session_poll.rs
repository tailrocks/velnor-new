//! Scripted poll. No sockets.

use std::collections::VecDeque;

use velnor_runner_github::{
    CAPACITY_HEADER, Certainty, Exchange, Method, ParsedBatch, Poll, RefreshGate, SessionError,
    SessionRequest, Transport, TransportFail, WireError, parse_poll, poll,
};

const QUEUE: &str = "_apis/runtime/runnerscalesets/7/sessions/s/messages";
const NULL_STATS: &str =
    r#"{"messageId":0,"messageType":"RunnerScaleSetJobMessages","body":"[]","statistics":null}"#;
const OMITTED_STATS: &str =
    r#"{"messageId":0,"messageType":"RunnerScaleSetJobMessages","body":"[]"}"#;
const POPULATED: &str = r#"{"messageId":1,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAvailable\",\"runnerRequestId\":1}]","statistics":{"totalAvailableJobs":1,"totalAcquiredJobs":0,"totalAssignedJobs":5,"totalRunningJobs":0,"totalRegisteredRunners":0,"totalBusyRunners":0,"totalIdleRunners":0}}"#;

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn once(status: u16, body: &str) -> Self {
        Self::replies(vec![Ok(exchange(status, body))])
    }

    fn replies(replies: Vec<Result<Exchange, TransportFail>>) -> Self {
        Self {
            replies: VecDeque::from(replies),
            seen: Vec::new(),
        }
    }
}

fn exchange(status: u16, body: &str) -> Exchange {
    Exchange {
        status,
        body: body.as_bytes().to_vec(),
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        self.replies.pop_front().ok_or(TransportFail::Reset)?
    }
}

fn header<'a>(request: &'a SessionRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn must_err<T>(result: &Result<T, SessionError>) -> Result<SessionError, &'static str> {
    match result {
        Ok(_) => Err("expected error"),
        Err(error) => Ok(*error),
    }
}

fn poll_batch(body: &str) -> Result<ParsedBatch, &'static str> {
    let mut script = Script::once(200, body);
    let polled =
        poll(&mut script, QUEUE, 0, 2, &RefreshGate::new(), || Ok(())).map_err(|_| "poll")?;
    assert_eq!(header(&script.seen[0], CAPACITY_HEADER), Some("2"));
    match polled {
        Poll::Batch(batch) => Ok(batch),
        Poll::Empty => Err("batch"),
    }
}

#[test]
fn poll_preserves_statistics() -> Result<(), &'static str> {
    let null = poll_batch(NULL_STATS)?;
    let omitted = poll_batch(OMITTED_STATS)?;
    assert_eq!(null.statistics, None);
    assert_eq!(omitted.statistics, None);
    assert_eq!(null.message_id, 0);
    let populated = poll_batch(POPULATED)?;
    let stats = populated.statistics.as_ref().ok_or("stats")?;
    assert_eq!(stats.assigned_population(), 5);
    assert_eq!(populated.jobs.len(), 1);
    Ok(())
}

#[test]
fn empty_poll_is_not_acknowledged() -> Result<(), &'static str> {
    let mut script = Script::replies(vec![Ok(exchange(202, NULL_STATS)), Ok(exchange(204, ""))]);
    let polled =
        poll(&mut script, QUEUE, 4, 9, &RefreshGate::new(), || Ok(())).map_err(|_| "poll")?;
    assert_eq!(polled, Poll::Empty);
    assert_eq!(script.seen.len(), 1);
    assert_eq!(script.seen[0].method, Method::Get);
    assert_eq!(header(&script.seen[0], CAPACITY_HEADER), Some("9"));
    Ok(())
}

#[test]
fn poll_query_omits_non_positive_cursor_and_sends_total_capacity() -> Result<(), &'static str> {
    for cursor in [-1_i64, 0, 1] {
        let mut script = Script::once(202, "");
        poll(
            &mut script,
            QUEUE,
            cursor,
            5,
            &RefreshGate::new(),
            || Ok(()),
        )
        .map_err(|_| "poll")?;
        let request = &script.seen[0];
        let query = request.query.as_deref().ok_or("query")?;
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.path, QUEUE);
        assert!(query.starts_with("api-version=6.0-preview"));
        assert_eq!(
            header(request, "Accept"),
            Some("application/json; api-version=6.0-preview")
        );
        assert_eq!(header(request, CAPACITY_HEADER), Some("5"));
        if cursor > 0 {
            assert_eq!(query, "api-version=6.0-preview&lastMessageId=1");
        } else {
            assert!(!query.contains("lastMessageId"));
        }
    }
    Ok(())
}

#[test]
fn poll_retries_unauthorized_once_then_reads() -> Result<(), &'static str> {
    let mut script = Script::replies(vec![
        Ok(exchange(401, "")),
        Ok(exchange(200, OMITTED_STATS)),
    ]);
    let mut refreshes = 0_u32;
    let gate = RefreshGate::new();
    let polled = poll(&mut script, QUEUE, 1, 2, &gate, || {
        refreshes += 1;
        Ok(())
    })
    .map_err(|_| "poll")?;
    assert!(matches!(polled, Poll::Batch(_)));
    assert_eq!(refreshes, 1);
    assert_eq!(script.seen.len(), 2);
    assert_eq!(script.seen[0], script.seen[1]);
    assert_eq!(gate.started().map_err(|_| "started")?, 1);
    Ok(())
}

#[test]
fn poll_second_unauthorized_fails_without_a_third_call() -> Result<(), &'static str> {
    let mut script = Script::replies(vec![
        Ok(exchange(401, "")),
        Ok(exchange(401, "")),
        Ok(exchange(200, OMITTED_STATS)),
    ]);
    let mut refreshes = 0_u32;
    let gate = RefreshGate::new();
    let err = must_err(&poll(&mut script, QUEUE, 0, 2, &gate, || {
        refreshes += 1;
        Ok(())
    }))?;
    assert_eq!(err, SessionError::Wire(WireError::RefreshExhausted));
    assert_eq!(err.certainty(), Certainty::Definite);
    assert_eq!(refreshes, 1);
    assert_eq!(script.seen.len(), 2);
    Ok(())
}

#[test]
fn poll_forbidden_does_not_retry() -> Result<(), &'static str> {
    let mut script = Script::replies(vec![
        Ok(exchange(403, "")),
        Ok(exchange(200, OMITTED_STATS)),
    ]);
    let mut refreshes = 0_u32;
    let gate = RefreshGate::new();
    let err = must_err(&poll(&mut script, QUEUE, -1, 2, &gate, || {
        refreshes += 1;
        Ok(())
    }))?;
    assert_eq!(err, SessionError::Wire(WireError::Forbidden));
    assert_eq!(refreshes, 0);
    assert_eq!(script.seen.len(), 1);
    assert_eq!(gate.started().map_err(|_| "started")?, 0);
    Ok(())
}

#[test]
fn null_and_omitted_statistics_decode_without_a_socket() -> Result<(), &'static str> {
    let null = match parse_poll(200, NULL_STATS).map_err(|_| "null")? {
        Poll::Batch(batch) => batch,
        Poll::Empty => return Err("batch"),
    };
    let omitted = match parse_poll(200, OMITTED_STATS).map_err(|_| "omitted")? {
        Poll::Batch(batch) => batch,
        Poll::Empty => return Err("batch"),
    };
    assert_eq!(null.statistics, omitted.statistics);
    Ok(())
}

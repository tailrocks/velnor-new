//! Scripted acquire, ack, JIT, and session delete. No sockets.

use std::collections::VecDeque;

use velnor_runner_github::{
    Ack, AckScope, AcquireOutcome, Certainty, Exchange, InnerKind, Method, ParsedBatch, Poll,
    RefreshGate, SessionError, SessionRequest, Transport, TransportFail, WireError, ack, acquire,
    acquire_path, delete_session, jit, jit_path, jit_request, may_ack, parse_poll,
};

const QUEUE: &str = "_apis/runtime/runnerscalesets/7/sessions/s/messages";
const UNKNOWN: &str = r#"{"messageId":4,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobExploded\",\"runnerRequestId\":9}]"}"#;
const AVAILABLE: &str = r#"{"messageId":4,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAvailable\",\"runnerRequestId\":3}]"}"#;
const TOKEN: &str = "queue-token-canary";
const ADMIN: &str = "admin-canary";

fn scope(replay_safe: bool, sole_unacquired_offer: bool) -> AckScope<'static> {
    AckScope {
        replay_safe,
        sole_unacquired_offer,
        queue_token: TOKEN,
    }
}

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

    fn fail(fail: TransportFail) -> Self {
        Self::replies(vec![Err(fail)])
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

fn batch_of(body: &str) -> Result<ParsedBatch, &'static str> {
    match parse_poll(200, body).map_err(|_| "batch")? {
        Poll::Batch(batch) => Ok(batch),
        Poll::Empty => Err("batch"),
    }
}

fn suppressed(
    batch: &ParsedBatch,
    replay_safe: bool,
    sole_unacquired_offer: bool,
) -> Result<(), &'static str> {
    let mut script = Script::once(204, "");
    let decision = ack(
        &mut script,
        QUEUE,
        batch,
        &scope(replay_safe, sole_unacquired_offer),
        &RefreshGate::new(),
        |_, _| Ok(()),
    )
    .map_err(|_| "ack")?;
    assert_eq!(decision, Ack::Suppressed);
    assert_eq!(script.seen.len(), 0);
    Ok(())
}

#[test]
fn acquire_keeps_partial_ids_and_rejects_foreign_ids() -> Result<(), &'static str> {
    let gate = RefreshGate::new();
    let mut script = Script::once(200, r#"{"count":2,"value":[1,3]}"#);
    let partial = acquire(&mut script, 3, &[1, 2, 3], &[], TOKEN, &gate, |_, _| Ok(()))
        .map_err(|_| "partial")?;
    assert_eq!(partial, AcquireOutcome::Acquired(vec![1, 3]));
    assert_eq!(script.seen[0].method, Method::Post);
    assert_eq!(script.seen[0].path, acquire_path(3));
    assert_eq!(&script.seen[0].body, b"[1,2,3]");
    assert_eq!(header(&script.seen[0], "X-ScaleSetMaxCapacity"), None);
    assert_eq!(
        header(&script.seen[0], "Authorization"),
        Some("Bearer queue-token-canary")
    );
    assert!(!format!("{:?}", script.seen[0]).contains(TOKEN));
    assert_eq!(header(&script.seen[0], "User-Agent"), Some("velnor-host"));
    assert_eq!(
        script.seen[0].query.as_deref(),
        Some("api-version=6.0-preview")
    );
    let mut foreign = Script::once(200, r#"{"count":1,"value":[9]}"#);
    let err = must_err(&acquire(
        &mut foreign,
        3,
        &[1],
        &[],
        TOKEN,
        &gate,
        |_, _| Ok(()),
    ))?;
    assert_eq!(err, SessionError::Uncertain);
    assert_eq!(err.certainty(), Certainty::Uncertain);
    let mut same = Script::once(200, r#"{"count":2,"value":[2,1]}"#);
    let noop =
        acquire(&mut same, 3, &[1, 2], &[1, 2], TOKEN, &gate, |_, _| Ok(())).map_err(|_| "noop")?;
    assert_eq!(noop, AcquireOutcome::Noop);
    let mut bad = Script::once(200, r#"{"count":1,"value":[1,2]}"#);
    let err = must_err(&acquire(&mut bad, 3, &[1, 2], &[], TOKEN, &gate, |_, _| {
        Ok(())
    }))?;
    assert_eq!(err, SessionError::Uncertain);
    assert_eq!(err.certainty(), Certainty::Uncertain);
    Ok(())
}

#[test]
fn acquire_timeout_and_reset_are_not_definite_failures() -> Result<(), &'static str> {
    let gate = RefreshGate::new();
    for fail in [TransportFail::Timeout, TransportFail::Reset] {
        let mut script = Script::fail(fail);
        let err = must_err(&acquire(&mut script, 3, &[1], &[], TOKEN, &gate, |_, _| {
            Ok(())
        }))?;
        assert_eq!(err, SessionError::Uncertain);
        assert_eq!(err.certainty(), Certainty::Uncertain);
        assert_eq!(gate.started().map_err(|_| "started")?, 0);
    }
    let mut script = Script::once(403, "");
    let err = must_err(&acquire(&mut script, 3, &[1], &[], TOKEN, &gate, |_, _| {
        Ok(())
    }))?;
    assert_eq!(err, SessionError::Wire(WireError::Forbidden));
    assert_eq!(err.certainty(), Certainty::Definite);
    assert_eq!(script.seen.len(), 1);
    Ok(())
}

#[test]
fn ack_skips_unsafe_batches_and_deletes_real_ids() -> Result<(), &'static str> {
    let unknown = batch_of(UNKNOWN)?;
    assert!(matches!(unknown.jobs[0].kind, InnerKind::Unsupported(_)));
    assert!(!may_ack(&unknown, true));
    suppressed(&unknown, true, false)?;
    let negative = ParsedBatch {
        message_id: -1,
        statistics: None,
        jobs: Vec::new(),
    };
    assert!(!may_ack(&negative, true));
    suppressed(&negative, true, false)?;
    let available = batch_of(AVAILABLE)?;
    assert!(may_ack(&available, true));
    suppressed(&available, true, true)?;
    suppressed(&available, false, false)?;
    let zero = ParsedBatch {
        message_id: 0,
        statistics: None,
        jobs: Vec::new(),
    };
    assert!(may_ack(&zero, true));
    let mut script = Script::once(204, "");
    let decision = ack(
        &mut script,
        QUEUE,
        &zero,
        &scope(true, false),
        &RefreshGate::new(),
        |_, _| Ok(()),
    )
    .map_err(|_| "ack")?;
    assert_eq!(decision, Ack::Deleted);
    assert_eq!(script.seen[0].method, Method::Delete);
    assert_eq!(script.seen[0].path, format!("{QUEUE}/0"));
    assert!(script.seen[0].query.is_none());
    let mut script = Script::once(204, "");
    let decision = ack(
        &mut script,
        QUEUE,
        &available,
        &scope(true, false),
        &RefreshGate::new(),
        |_, _| Ok(()),
    )
    .map_err(|_| "ack")?;
    assert_eq!(decision, Ack::Deleted);
    assert_eq!(script.seen[0].path, format!("{QUEUE}/4"));
    Ok(())
}

#[test]
fn ack_non_204_fails_and_unauthorized_retries_once() -> Result<(), &'static str> {
    let zero = ParsedBatch {
        message_id: 0,
        statistics: None,
        jobs: Vec::new(),
    };
    let mut script = Script::once(200, "");
    let err = must_err(&ack(
        &mut script,
        QUEUE,
        &zero,
        &scope(true, false),
        &RefreshGate::new(),
        |_, _| Ok(()),
    ))?;
    assert_eq!(err, SessionError::Wire(WireError::UnexpectedStatus));
    assert_eq!(script.seen.len(), 1);
    let mut script = Script::replies(vec![
        Ok(exchange(401, "")),
        Ok(exchange(401, "")),
        Ok(exchange(204, "")),
    ]);
    let mut refreshes = 0_u32;
    let err = must_err(&ack(
        &mut script,
        QUEUE,
        &zero,
        &scope(true, false),
        &RefreshGate::new(),
        |_, _| {
            refreshes += 1;
            Ok(())
        },
    ))?;
    assert_eq!(err, SessionError::Wire(WireError::RefreshExhausted));
    assert_eq!(refreshes, 1);
    assert_eq!(script.seen.len(), 2);
    assert!(
        script
            .seen
            .iter()
            .all(|request| request.method == Method::Delete)
    );
    Ok(())
}

#[test]
fn jit_bytes_stay_out_of_debug_and_errors() -> Result<(), &'static str> {
    let request_canary = "request-jit-canary";
    let response_canary = "response-jit-canary";
    let request = format!(r#"{{"name":"{request_canary}"}}"#);
    let response = format!(r#"{{"encodedJITConfig":"{response_canary}"}}"#);
    let mut script = Script::once(200, &response);
    let config = jit(&mut script, 7, ADMIN, request.as_bytes()).map_err(|_| "jit")?;
    assert_eq!(config.expose(), response_canary);
    assert!(!config.expose().contains("encodedJITConfig"));
    assert!(!format!("{config:?}").contains(response_canary));
    let rendered = format!("{:?}", script.seen[0]);
    assert!(!rendered.contains(request_canary));
    assert!(!rendered.contains(response_canary));
    assert!(!rendered.contains(ADMIN));
    assert_eq!(
        header(&script.seen[0], "Authorization"),
        Some("Bearer admin-canary")
    );
    assert_eq!(header(&script.seen[0], "User-Agent"), Some("velnor-host"));
    assert_eq!(script.seen[0].body, request.as_bytes());
    assert_eq!(script.seen[0].method, Method::Post);
    assert_eq!(script.seen[0].path, jit_path(7));
    assert_eq!(script.seen.len(), 1);
    let mut script = Script::once(500, response_canary);
    let err = must_err(&jit(&mut script, 7, ADMIN, request.as_bytes()))?;
    let rendered = format!("{err} {err:?} {:?}", script.seen[0]);
    assert!(!rendered.contains(request_canary));
    assert!(!rendered.contains(response_canary));
    assert_eq!(script.seen.len(), 1);
    assert_eq!(err.certainty(), Certainty::Uncertain);
    let mut malformed = Script::once(200, r#"{"encodedJITConfig":""}"#);
    let err = must_err(&jit(&mut malformed, 7, ADMIN, request.as_bytes()))?;
    assert_eq!(err, SessionError::Uncertain);
    assert_eq!(err.certainty(), Certainty::Uncertain);
    Ok(())
}

#[test]
fn delete_session_rejects_non_204() -> Result<(), &'static str> {
    let cases = [
        (
            200,
            SessionError::Wire(WireError::UnexpectedStatus),
            Certainty::Uncertain,
        ),
        (
            401,
            SessionError::Wire(WireError::UnexpectedStatus),
            Certainty::Uncertain,
        ),
        (
            403,
            SessionError::Wire(WireError::Forbidden),
            Certainty::Definite,
        ),
        (409, SessionError::Conflict, Certainty::Definite),
        (
            500,
            SessionError::Wire(WireError::UnexpectedStatus),
            Certainty::Uncertain,
        ),
    ];
    for (status, expected, certainty) in cases {
        let mut script = Script::once(status, "delete-body-canary");
        let err = must_err(&delete_session(&mut script, 7, "sess", ADMIN))?;
        assert_eq!(err, expected);
        assert_eq!(err.certainty(), certainty);
        assert_eq!(script.seen.len(), 1);
        assert_eq!(script.seen[0].method, Method::Delete);
        assert!(script.seen[0].path.ends_with("/7/sessions/sess"));
        assert_eq!(
            script.seen[0].query.as_deref(),
            Some("api-version=6.0-preview")
        );
        assert!(!format!("{err} {err:?}").contains("delete-body-canary"));
    }
    let mut script = Script::once(204, "");
    delete_session(&mut script, 7, "sess", ADMIN).map_err(|_| "deleted")?;
    assert_eq!(
        header(&script.seen[0], "Authorization"),
        Some("Bearer admin-canary")
    );
    let mut script = Script::fail(TransportFail::Timeout);
    let err = must_err(&delete_session(&mut script, 7, "sess", ADMIN))?;
    assert_eq!(err, SessionError::Uncertain);
    Ok(())
}

#[test]
fn jit_request_uses_runner_image_work_folder() -> Result<(), WireError> {
    let body = jit_request("runner-ab")?;
    assert_eq!(body, br#"{"name":"runner-ab","workFolder":"_work"}"#);
    assert_eq!(jit_request(""), Err(WireError::RegistrationRejected));
    assert_eq!(jit_request("a/b"), Err(WireError::RegistrationRejected));
    assert_eq!(jit_request("a b"), Err(WireError::RegistrationRejected));
    Ok(())
}

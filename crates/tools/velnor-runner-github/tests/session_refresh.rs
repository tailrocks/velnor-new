//! A queue-token 401 refreshes the current session and rebuilds the replay.

use std::collections::VecDeque;

use velnor_runner_github::{
    Ack, AckScope, Exchange, Method, ParsedBatch, Poll, QueueSession, RefreshGate, SessionError,
    SessionRequest, Transport, TransportFail, WireError, ack, acquire, create_session, poll,
    refresh_queue_request,
};
use zeroize::Zeroize;

const ADMIN_ORIGIN: &str = "https://actions.example";
const NEW_QUEUE_URL: &str =
    "https://queue-new.example/_apis/runtime/runnerscalesets/7/sessions/sess/messages";
const POLL_BODY: &str = r#"{"messageId":4,"messageType":"RunnerScaleSetJobMessages","body":"[]"}"#;
const ACQUIRE_BODY: &str = r#"{"count":1,"value":[8]}"#;
const OLD_SESSION: &str = r#"{"sessionId":"sess","messageQueueUrl":"https://queue-old.example/_apis/runtime/runnerscalesets/7/sessions/sess/messages","messageQueueAccessToken":"expired-queue-canary"}"#;
const NEW_SESSION: &str = r#"{"sessionId":"sess","messageQueueUrl":"https://queue-new.example/_apis/runtime/runnerscalesets/7/sessions/sess/messages","messageQueueAccessToken":"replacement-queue-canary"}"#;

struct RoutedScript {
    origin: String,
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<(String, SessionRequest)>,
}

impl RoutedScript {
    fn replies(statuses: &[(u16, &str)]) -> Self {
        Self {
            origin: "https://queue-old.example".to_owned(),
            replies: statuses
                .iter()
                .map(|(status, body)| {
                    Ok(Exchange {
                        status: *status,
                        body: body.as_bytes().to_vec(),
                    })
                })
                .collect(),
            seen: Vec::new(),
        }
    }
}

impl Transport for RoutedScript {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push((self.origin.clone(), request.clone()));
        self.replies
            .pop_front()
            .unwrap_or(Err(TransportFail::Reset))
    }
}

fn create_queue_session() -> Result<QueueSession, &'static str> {
    let mut script = RoutedScript::replies(&[(200, OLD_SESSION)]);
    create_session(&mut script, 7, "velnor-host", "admin-canary").map_err(|_| "create")
}

fn route_queue(
    url: &str,
    transport: &mut RoutedScript,
    request: &mut SessionRequest,
) -> Result<(), SessionError> {
    let Some(rest) = url.strip_prefix("https://") else {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    };
    let Some((host, path)) = rest.split_once('/') else {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    };
    if host.is_empty() || host.contains('@') || host.chars().any(char::is_whitespace) {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    transport.origin = format!("https://{host}");
    path.clone_into(&mut request.path);
    Ok(())
}

fn route_ack_queue(
    url: &str,
    transport: &mut RoutedScript,
    request: &mut SessionRequest,
    message_id: i64,
) -> Result<(), SessionError> {
    let suffix = format!("/{message_id}");
    if !request.path.ends_with(&suffix) {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    route_queue(url, transport, request)?;
    request.path.push_str(&suffix);
    Ok(())
}

fn refresh_callback(
    scale_set_id: i64,
    session: &mut QueueSession,
) -> impl FnMut(&mut RoutedScript, &mut SessionRequest) -> Result<(), SessionError> + '_ {
    move |transport, request| {
        ADMIN_ORIGIN.clone_into(&mut transport.origin);
        let queue_url =
            refresh_queue_request(transport, scale_set_id, session, "admin-canary", request)?;
        route_queue(queue_url, transport, request)
    }
}

#[test]
fn poll_refreshes_same_session_and_replays_on_new_queue_origin() -> Result<(), &'static str> {
    let mut session = create_queue_session()?;
    let mut token = session.token().to_owned();
    let mut script = RoutedScript::replies(&[(401, ""), (200, NEW_SESSION), (200, POLL_BODY)]);
    let polled = {
        let mut refresh = refresh_callback(7, &mut session);
        poll(
            &mut script,
            "_apis/runtime/runnerscalesets/7/sessions/sess/messages",
            3,
            2,
            &token,
            &RefreshGate::new(),
            &mut refresh,
        )
        .map_err(|_| "poll")?
    };
    token.zeroize();

    assert!(matches!(polled, Poll::Batch(_)));
    assert_eq!(session.session_id, "sess");
    assert_eq!(session.message_queue_url, NEW_QUEUE_URL);
    assert_eq!(session.token(), "replacement-queue-canary");
    assert_eq!(script.seen.len(), 3);

    let (old_origin, first) = &script.seen[0];
    let (admin_origin, patch) = &script.seen[1];
    let (new_origin, replay) = &script.seen[2];
    assert_eq!(old_origin, "https://queue-old.example");
    assert_eq!(first.method, Method::Get);
    assert_eq!(
        first.path,
        "_apis/runtime/runnerscalesets/7/sessions/sess/messages"
    );
    assert_eq!(
        first.query.as_deref(),
        Some("api-version=6.0-preview&lastMessageId=3")
    );
    assert_eq!(admin_origin, ADMIN_ORIGIN);
    assert_eq!(patch.method, Method::Patch);
    assert!(patch.path.ends_with("/7/sessions/sess"));
    assert_eq!(new_origin, "https://queue-new.example");
    assert_eq!(replay.method, first.method);
    assert_eq!(
        replay.path,
        "_apis/runtime/runnerscalesets/7/sessions/sess/messages"
    );
    assert_eq!(replay.query, first.query);
    assert_eq!(
        header(first, "Authorization"),
        Some("Bearer expired-queue-canary")
    );
    assert_eq!(header(patch, "Authorization"), Some("Bearer admin-canary"));
    assert_eq!(
        header(replay, "Authorization"),
        Some("Bearer replacement-queue-canary")
    );
    let rendered = format!("{session:?} {first:?} {patch:?} {replay:?}");
    assert!(!rendered.contains("expired-queue-canary"));
    assert!(!rendered.contains("replacement-queue-canary"));
    assert!(!rendered.contains("admin-canary"));
    Ok(())
}

#[test]
fn second_401_is_exhausted_after_one_session_refresh() -> Result<(), &'static str> {
    let mut session = create_queue_session()?;
    let mut token = session.token().to_owned();
    let mut script =
        RoutedScript::replies(&[(401, ""), (200, NEW_SESSION), (401, ""), (200, POLL_BODY)]);
    let result = {
        let mut refresh = refresh_callback(7, &mut session);
        poll(
            &mut script,
            "_apis/runtime/runnerscalesets/7/sessions/sess/messages",
            0,
            2,
            &token,
            &RefreshGate::new(),
            &mut refresh,
        )
    };
    token.zeroize();

    assert_eq!(result, Err(SessionError::Wire(WireError::RefreshExhausted)));
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.replies.len(), 1);
    assert_eq!(script.seen[1].1.method, Method::Patch);
    assert_eq!(script.seen[2].1.method, Method::Get);
    assert_eq!(
        header(&script.seen[2].1, "Authorization"),
        Some("Bearer replacement-queue-canary")
    );
    assert_eq!(session.token(), "replacement-queue-canary");
    let rendered = format!("{:?} {:?}", script.seen[1].1, script.seen[2].1);
    assert!(!rendered.contains("replacement-queue-canary"));
    assert!(!format!("{result:?}").contains("replacement-queue-canary"));
    Ok(())
}

#[test]
fn acquire_refresh_replays_same_admin_operation_with_new_queue_token() -> Result<(), &'static str> {
    let mut session = create_queue_session()?;
    let mut queue_token = session.token().to_owned();
    let mut script = RoutedScript::replies(&[(401, ""), (200, NEW_SESSION), (200, ACQUIRE_BODY)]);
    ADMIN_ORIGIN.clone_into(&mut script.origin);
    let acquired = {
        let mut refresh = |transport: &mut RoutedScript, request: &mut SessionRequest| {
            ADMIN_ORIGIN.clone_into(&mut transport.origin);
            let _queue_url =
                refresh_queue_request(transport, 7, &mut session, "admin-canary", request)?;
            ADMIN_ORIGIN.clone_into(&mut transport.origin);
            Ok(())
        };
        acquire(
            &mut script,
            7,
            &[8],
            &[],
            &queue_token,
            &RefreshGate::new(),
            &mut refresh,
        )
        .map_err(|_| "acquire")?
    };
    queue_token.zeroize();

    assert_eq!(
        acquired,
        velnor_runner_github::AcquireOutcome::Acquired(vec![8])
    );
    assert_eq!(script.seen.len(), 3);
    let first = &script.seen[0].1;
    let patch = &script.seen[1].1;
    let replay = &script.seen[2].1;
    assert_eq!(script.seen[0].0, ADMIN_ORIGIN);
    assert_eq!(script.seen[1].0, ADMIN_ORIGIN);
    assert_eq!(script.seen[2].0, ADMIN_ORIGIN);
    assert_eq!(first.method, Method::Post);
    assert_eq!(first.path, "_apis/runtime/runnerscalesets/7/acquirejobs");
    assert_eq!(patch.method, Method::Patch);
    assert_eq!(replay.method, first.method);
    assert_eq!(replay.path, first.path);
    assert_eq!(replay.body, first.body);
    assert_eq!(
        header(first, "Authorization"),
        Some("Bearer expired-queue-canary")
    );
    assert_eq!(header(patch, "Authorization"), Some("Bearer admin-canary"));
    assert_eq!(
        header(replay, "Authorization"),
        Some("Bearer replacement-queue-canary")
    );
    assert_eq!(session.message_queue_url, NEW_QUEUE_URL);
    assert_eq!(session.token(), "replacement-queue-canary");
    Ok(())
}

#[test]
fn ack_refresh_routes_delete_to_new_queue_endpoint() -> Result<(), &'static str> {
    let mut session = create_queue_session()?;
    let mut queue_token = session.token().to_owned();
    let mut script = RoutedScript::replies(&[(401, ""), (200, NEW_SESSION), (204, "")]);
    let batch = ParsedBatch {
        message_id: 7,
        statistics: None,
        jobs: Vec::new(),
    };
    let acked = {
        let mut refresh = |transport: &mut RoutedScript, request: &mut SessionRequest| {
            ADMIN_ORIGIN.clone_into(&mut transport.origin);
            let queue_url =
                refresh_queue_request(transport, 7, &mut session, "admin-canary", request)?;
            route_ack_queue(queue_url, transport, request, batch.message_id)
        };
        ack(
            &mut script,
            "_apis/runtime/runnerscalesets/7/sessions/sess/messages",
            &batch,
            &AckScope {
                replay_safe: true,
                sole_unacquired_offer: false,
                queue_token: &queue_token,
            },
            &RefreshGate::new(),
            &mut refresh,
        )
        .map_err(|_| "ack")?
    };
    queue_token.zeroize();

    assert_eq!(acked, Ack::Deleted);
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.seen[0].0, "https://queue-old.example");
    assert_eq!(script.seen[1].0, ADMIN_ORIGIN);
    assert_eq!(script.seen[2].0, "https://queue-new.example");
    assert_eq!(script.seen[0].1.method, Method::Delete);
    assert_eq!(
        script.seen[0].1.path,
        "_apis/runtime/runnerscalesets/7/sessions/sess/messages/7"
    );
    assert_eq!(script.seen[2].1.method, Method::Delete);
    assert_eq!(script.seen[2].1.path, script.seen[0].1.path);
    assert_eq!(
        header(&script.seen[2].1, "Authorization"),
        Some("Bearer replacement-queue-canary")
    );
    assert_eq!(session.message_queue_url, NEW_QUEUE_URL);
    assert_eq!(session.token(), "replacement-queue-canary");
    Ok(())
}

fn header<'a>(request: &'a SessionRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

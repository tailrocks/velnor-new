//! Restart opens a session by deleting only the ids it was given.

use std::collections::VecDeque;

use velnor_runner_github::{
    Exchange, Method, SessionError, SessionRequest, Transport, TransportFail, reopen_session,
};

const BODY: &str = r#"{"sessionId":"sess","messageQueueUrl":"_apis/runtime/runnerscalesets/7/sessions/sess/messages","messageQueueAccessToken":"queue-token-canary","ownerName":"velnor-host"}"#;

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn replies(replies: Vec<(u16, &str)>) -> Self {
        Self {
            replies: replies
                .into_iter()
                .map(|(status, body)| {
                    Ok(Exchange {
                        status,
                        body: body.as_bytes().to_vec(),
                    })
                })
                .collect(),
            seen: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        self.replies.pop_front().ok_or(TransportFail::Reset)?
    }
}

#[test]
fn reopen_deletes_only_listed_sessions_then_creates() -> Result<(), &'static str> {
    let mut script = Script::replies(vec![(204, ""), (404, "missing"), (200, BODY)]);
    let opened = reopen_session(
        &mut script,
        7,
        "velnor-host",
        "admin-canary",
        &["one-leaked", "two-leaked"],
    )
    .map_err(|_| "reopen")?;
    assert_eq!(opened.session_id, "sess");
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.seen[0].method, Method::Delete);
    assert!(script.seen[0].path.ends_with("/7/sessions/one-leaked"));
    assert_eq!(script.seen[1].method, Method::Delete);
    assert!(script.seen[1].path.ends_with("/7/sessions/two-leaked"));
    assert_eq!(script.seen[2].method, Method::Post);
    assert!(script.seen[2].path.ends_with("/7/sessions"));
    assert!(!script.seen[2].path.contains("one-leaked"));
    assert!(!script.seen[2].path.contains("two-leaked"));
    assert!(!format!("{opened:?}").contains("queue-token-canary"));
    Ok(())
}

#[test]
fn reopen_treats_expired_session_400_as_gone() -> Result<(), &'static str> {
    let expired = r#"{"message":"The session identifier dead-session is not valid.","typeName":"GitHub.Actions.Runtime.WebApi.RunnerScaleSetSessionExpiredException, GitHub.Actions.Runtime.WebApi"}"#;
    let mut script = Script::replies(vec![(400, expired), (200, BODY)]);
    let opened = reopen_session(
        &mut script,
        7,
        "velnor-host",
        "admin-canary",
        &["dead-session"],
    )
    .map_err(|_| "reopen")?;
    assert_eq!(opened.session_id, "sess");
    assert_eq!(script.seen.len(), 2);
    assert_eq!(script.seen[0].method, Method::Delete);
    assert!(script.seen[0].path.ends_with("/7/sessions/dead-session"));
    assert_eq!(script.seen[1].method, Method::Post);
    assert!(script.seen[1].path.ends_with("/7/sessions"));
    let rendered = format!("{opened:?}");
    assert!(!rendered.contains("dead-session"));
    assert!(!rendered.contains("RunnerScaleSetSessionExpiredException"));
    Ok(())
}

#[test]
fn reopen_rejects_unrelated_session_400() -> Result<(), &'static str> {
    let mut script = Script::replies(vec![(400, "other-canary")]);
    let opened = reopen_session(
        &mut script,
        7,
        "velnor-host",
        "admin-canary",
        &["dead-session"],
    );
    match opened {
        Err(error) => assert_eq!(
            error,
            SessionError::Wire(velnor_runner_github::WireError::UnexpectedStatus)
        ),
        Ok(_) => return Err("expected refusal"),
    }
    assert_eq!(script.seen.len(), 1);
    assert_eq!(script.seen[0].method, Method::Delete);
    assert!(!format!("{:?}", script.seen[0]).contains("other-canary"));
    Ok(())
}

#[test]
fn reopen_conflict_does_not_delete_an_unlisted_session() -> Result<(), &'static str> {
    let mut script = Script::replies(vec![(409, "other-session")]);
    let opened = reopen_session(&mut script, 7, "velnor-host", "admin-canary", &[]);
    match opened {
        Err(error) => assert_eq!(error, SessionError::Conflict),
        Ok(_) => return Err("expected conflict"),
    }
    assert_eq!(script.seen.len(), 1);
    assert_eq!(script.seen[0].method, Method::Post);
    assert!(!format!("{:?}", script.seen[0]).contains("other-session"));
    Ok(())
}

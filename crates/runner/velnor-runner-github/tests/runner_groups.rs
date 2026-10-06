//! Runner group list. No sockets.

use std::collections::VecDeque;

use velnor_runner_github::{
    Exchange, Method, SessionError, SessionRequest, Transport, TransportFail, WireError,
    list_runner_groups,
};

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn once(status: u16, body: &str) -> Self {
        Self {
            replies: VecDeque::from(vec![Ok(Exchange {
                status,
                body: body.as_bytes().to_vec(),
            })]),
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
fn empty_token_does_not_call() -> Result<(), &'static str> {
    let mut script = Script::once(200, r#"{"count":0,"value":[]}"#);
    match list_runner_groups(&mut script, "") {
        Err(err) => assert_eq!(err, SessionError::Wire(WireError::RegistrationRejected)),
        Ok(_) => return Err("expected reject"),
    }
    assert_eq!(script.seen.len(), 0);
    Ok(())
}

#[test]
fn list_reads_default_group_and_hides_the_token() -> Result<(), &'static str> {
    let body = r#"{"count":1,"value":[{"id":1,"name":"Default","isDefaultGroup":true}]}"#;
    let mut script = Script::once(200, body);
    let groups = list_runner_groups(&mut script, "admin-canary").map_err(|_| "list")?;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, 1);
    assert_eq!(groups[0].name, "Default");
    assert!(groups[0].is_default);
    assert_eq!(script.seen[0].method, Method::Get);
    assert_eq!(script.seen[0].path, "_apis/runtime/runnergroups");
    assert_eq!(
        script.seen[0].query.as_deref(),
        Some("api-version=6.0-preview")
    );
    assert!(!format!("{:?}", script.seen[0]).contains("admin-canary"));
    Ok(())
}

#[test]
fn count_mismatch_is_malformed() -> Result<(), &'static str> {
    let mut script = Script::once(200, r#"{"count":2,"value":[]}"#);
    match list_runner_groups(&mut script, "admin-canary") {
        Err(err) => assert_eq!(err, SessionError::Wire(WireError::Malformed)),
        Ok(_) => return Err("expected malformed"),
    }
    Ok(())
}

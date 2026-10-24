use std::collections::VecDeque;

use velnor_runner_github::{
    Exchange, Method, SessionError, SessionRequest, Transport, TransportFail, WireError,
};

use crate::{https::HttpsTransport, scale_set::EnsureError};

use super::{
    AdminEndpointTransport, Recording, confirm_runner_absence, map_session, register_repository,
};

#[test]
fn uncertain_remote_result_stays_uncertain() {
    let transport = recording();
    assert_eq!(
        map_session(SessionError::Uncertain, &transport),
        EnsureError::Uncertain
    );
}

#[test]
fn forbidden_remote_result_stays_explicit() {
    let transport = recording();
    assert_eq!(
        map_session(SessionError::Wire(WireError::Forbidden), &transport),
        EnsureError::Forbidden
    );
}

#[test]
fn unexpected_status_keeps_its_bounded_stage() {
    let mut transport = recording();
    transport.status = 502;
    transport.step = "runner-absence";
    assert_eq!(
        map_session(SessionError::Wire(WireError::UnexpectedStatus), &transport),
        EnsureError::Unexpected {
            status: 502,
            step: "runner-absence",
        }
    );
}

fn recording() -> Recording {
    Recording::new(
        HttpsTransport::new("https://api.github.com")
            .expect("fixed GitHub API is an HTTPS endpoint"),
    )
}

struct CapturedRequest {
    base: String,
    request: SessionRequest,
}

struct Script {
    base: String,
    replies: VecDeque<Exchange>,
    seen: Vec<CapturedRequest>,
}

impl Script {
    fn new(replies: &[(u16, &str)]) -> Self {
        Self {
            base: "https://api.github.com".to_owned(),
            replies: replies
                .iter()
                .map(|(status, body)| Exchange {
                    status: *status,
                    body: body.as_bytes().to_vec(),
                })
                .collect(),
            seen: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(CapturedRequest {
            base: self.base.clone(),
            request: request.clone(),
        });
        self.replies.pop_front().ok_or(TransportFail::Reset)
    }
}

impl AdminEndpointTransport for Script {
    fn set_base(&mut self, base: &str) -> Result<(), EnsureError> {
        self.base = base.to_owned();
        Ok(())
    }
}

#[test]
fn admin_service_url_and_token_are_used_for_distinct_authorities() -> Result<(), String> {
    const SERVICE: &str = "https://actions.example/tenant";
    const ADMIN_TOKEN: &str = "admin-token-canary";
    let binding =
        crate::journal::request_lifecycle::tests::helpers::binding("remote-absence-test", 101)
            .map_err(|_| "binding")?;
    let parts = super::super::repository::RepositoryParts {
        owner: "owner-101",
        name: "repo-101",
        target: velnor_runner_github::workflow_run::RepositoryTarget {
            id: 101,
            owner: "owner-101",
            name: "repo-101",
        },
    };
    let mut transport = Recording::new(Script::new(&[
        (201, r#"{"token":"registration-token"}"#),
        (
            200,
            r#"{"url":"https://actions.example/tenant","token":"admin-token-canary"}"#,
        ),
        (
            200,
            r#"{"count":1,"value":[{"id":7,"name":"integration","labels":[{"name":"velnor","type":"System"},{"name":"integration","type":"System"}],"runnerSetting":{"disableUpdate":true}}]}"#,
        ),
        (200, r#"{"count":0,"value":[]}"#),
    ]));
    let admin = register_repository(&mut transport, &parts, "repository-pat-canary")
        .map_err(|_| "admin registration")?;
    transport
        .set_base(admin.expose_url())
        .map_err(|_| "endpoint")?;
    let prepared = crate::journal::PreparedRetirementRemoteAbsence::test_fixture();
    confirm_runner_absence(&mut transport, &binding, &prepared, &admin)
        .map_err(|_| "runner absence")?;

    assert_eq!(admin.expose_url(), SERVICE);
    assert_eq!(admin.expose_token(), ADMIN_TOKEN);
    assert_eq!(transport.inner.seen.len(), 4);
    let set_request = &transport.inner.seen[2];
    let runner_request = &transport.inner.seen[3];
    assert_eq!(set_request.base, SERVICE);
    assert_eq!(runner_request.base, SERVICE);
    assert_eq!(set_request.request.method, Method::Get);
    assert_eq!(runner_request.request.method, Method::Get);
    assert_eq!(
        header(&set_request.request, "Authorization"),
        Some("Bearer admin-token-canary")
    );
    assert_eq!(
        header(&runner_request.request, "Authorization"),
        Some("Bearer admin-token-canary")
    );
    assert!(!format!("{:?}", set_request.request).contains(ADMIN_TOKEN));
    Ok(())
}

fn header<'a>(request: &'a SessionRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

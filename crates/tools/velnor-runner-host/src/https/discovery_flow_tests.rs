use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use velnor_runner_github::{
    DiscoveryCredentialOutcome, DiscoveryCredentialStep, DiscoveryIntentId, DiscoveryIntentStore,
    ScaleSetFound, SessionError, exchange_repository_discovery_admin_once,
    issue_repository_discovery_token, read_repository_admin_evidence,
};

use super::{BoundedDiscoveryTransport, http_response, read_headers};

#[test]
fn full_auth_only_discovery_uses_bounded_exact_routes_and_durable_intents() {
    let (base, server) = successful_discovery_server();
    let mut transport =
        BoundedDiscoveryTransport::for_test(base, 32 * 1024, Duration::from_secs(3));
    let mut intents = RecordedIntents::default();

    let evidence = read_repository_admin_evidence(&mut transport, "acme", "widget", "host-secret")
        .expect("repository admin evidence should be parsed");
    assert_eq!(evidence.repository_id(), 22);
    assert_eq!(evidence.full_name(), "acme/widget");

    let registration =
        issue_repository_discovery_token(&mut transport, evidence, "host-secret", &mut intents)
            .expect("one repository token should be issued");
    let admin =
        exchange_repository_discovery_admin_once(&mut transport, registration, &mut intents)
            .expect("one Actions discovery credential should be issued");
    let groups = admin
        .list_runner_groups(&mut transport)
        .expect("runner groups should be readable");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, 19);
    assert_eq!(groups[0].name, "Velnor");

    assert_eq!(
        admin
            .get_existing_product_scale_set(&mut transport, 19, "ubuntu-24.04-scale-set")
            .expect("existing Scale Set lookup should be read-only"),
        ScaleSetFound::NotFound
    );

    assert_eq!(
        intents.events,
        [
            IntentEvent::Before(
                DiscoveryCredentialStep::RepositoryRegistrationToken,
                22,
                "acme/widget".to_owned(),
            ),
            IntentEvent::Outcome(1, DiscoveryCredentialOutcome::Succeeded,),
            IntentEvent::Before(
                DiscoveryCredentialStep::ActionsAdminExchange,
                22,
                "acme/widget".to_owned(),
            ),
            IntentEvent::Outcome(2, DiscoveryCredentialOutcome::Succeeded,),
        ]
    );

    let requests = server.join().expect("test server thread should finish");
    assert_successful_requests(&requests);
}

pub(super) fn assert_successful_requests(requests: &[RequestSummary]) {
    assert_eq!(
        requests,
        [
            RequestSummary::new("GET", "/repos/acme/widget", "Bearer", true, 0, false),
            RequestSummary::new(
                "POST",
                "/repos/acme/widget/actions/runners/registration-token",
                "Bearer",
                true,
                0,
                false,
            ),
            RequestSummary::new(
                "POST",
                "/actions/runner-registration",
                "RemoteAuth",
                true,
                66,
                true,
            ),
            RequestSummary::new(
                "GET",
                "/tenant-9/_apis/runtime/runnergroups?api-version=6.0-preview",
                "Bearer",
                true,
                0,
                false,
            ),
            RequestSummary::new(
                "GET",
                "/tenant-9/_apis/runtime/runnerscalesets?api-version=6.0-preview&name=ubuntu-24.04-scale-set&runnerGroupId=19",
                "Bearer",
                true,
                0,
                false,
            ),
        ]
    );
}

#[derive(Debug, PartialEq, Eq)]
enum IntentEvent {
    Before(DiscoveryCredentialStep, i64, String),
    Outcome(u64, DiscoveryCredentialOutcome),
}

#[derive(Default)]
struct RecordedIntents {
    next_id: u64,
    events: Vec<IntentEvent>,
}

impl DiscoveryIntentStore for RecordedIntents {
    fn persist_before(
        &mut self,
        step: DiscoveryCredentialStep,
        repository_id: i64,
        full_name: &str,
    ) -> Result<DiscoveryIntentId, SessionError> {
        self.next_id += 1;
        self.events.push(IntentEvent::Before(
            step,
            repository_id,
            full_name.to_owned(),
        ));
        Ok(DiscoveryIntentId::new(self.next_id).expect("positive intent id"))
    }

    fn record_outcome(
        &mut self,
        id: DiscoveryIntentId,
        outcome: DiscoveryCredentialOutcome,
    ) -> Result<(), SessionError> {
        self.events.push(IntentEvent::Outcome(id.get(), outcome));
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RequestSummary {
    method: String,
    target: String,
    authorization_scheme: &'static str,
    credential_matches: bool,
    body_bytes: usize,
    expected_admin_body: bool,
}

impl RequestSummary {
    fn new(
        method: &str,
        target: &str,
        authorization_scheme: &'static str,
        credential_matches: bool,
        body_bytes: usize,
        expected_admin_body: bool,
    ) -> Self {
        Self {
            method: method.to_owned(),
            target: target.to_owned(),
            authorization_scheme,
            credential_matches,
            body_bytes,
            expected_admin_body,
        }
    }
}

pub(super) fn successful_discovery_server() -> (String, thread::JoinHandle<Vec<RequestSummary>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener should bind");
    listener
        .set_nonblocking(true)
        .expect("test listener should be nonblocking");
    let base = format!("http://{}", listener.local_addr().expect("local address"));
    let responses: [(u16, &[u8], &str, &str); 5] = [
        (
            200,
            br#"{"id":22,"full_name":"acme/widget","private":true,"permissions":{"admin":true}}"#,
            "Bearer",
            "host-secret",
        ),
        (
            201,
            br#"{"token":"registration-secret"}"#,
            "Bearer",
            "host-secret",
        ),
        (
            200,
            br#"{"url":"https://pipelinesghubeus13.actions.githubusercontent.com/tenant-9","token":"admin-secret"}"#,
            "RemoteAuth",
            "registration-secret",
        ),
        (
            200,
            br#"{"count":1,"value":[{"id":19,"name":"Velnor","isDefaultGroup":true}]}"#,
            "Bearer",
            "admin-secret",
        ),
        (
            200,
            br#"{"count":0,"value":[]}"#,
            "Bearer",
            "admin-secret",
        ),
    ];
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut requests = Vec::with_capacity(responses.len());
        for (status, body, expected_scheme, expected_credential) in responses {
            let (mut stream, _) = accept_until(&listener, deadline);
            let summary = read_request_summary(&mut stream, expected_scheme, expected_credential);
            let accepted =
                summary.authorization_scheme == expected_scheme && summary.credential_matches;
            let response = if accepted {
                http_response(status, body)
            } else {
                http_response(401, b"unauthorized")
            };
            stream
                .write_all(&response)
                .expect("test response should write");
            requests.push(summary);
        }
        requests
    });
    (base, handle)
}

fn accept_until(listener: &TcpListener, deadline: Instant) -> (TcpStream, std::net::SocketAddr) {
    loop {
        match listener.accept() {
            Ok(connection) => return connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "discovery request deadline elapsed"
                );
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("loopback test listener failed: {error}"),
        }
    }
}

fn read_request_summary(
    stream: &mut TcpStream,
    expected_scheme: &str,
    expected_credential: &str,
) -> RequestSummary {
    let headers = read_headers(stream);
    let text = std::str::from_utf8(&headers).expect("HTTP headers are ASCII");
    let mut lines = text.split("\r\n");
    let mut request_line = lines
        .next()
        .expect("request line is present")
        .split_whitespace();
    let method = request_line.next().expect("method is present").to_owned();
    let target = request_line.next().expect("target is present").to_owned();
    let mut auth = None;
    let mut content_length = 0;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("authorization") {
            auth = Some(value.trim());
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<usize>().expect("valid body length");
        }
    }
    let mut body = vec![0; content_length];
    stream
        .read_exact(&mut body)
        .expect("declared request body should arrive");
    let (authorization_scheme, credential_matches) =
        classify_authorization(auth, expected_scheme, expected_credential);
    let expected_admin_body =
        body == br#"{"url":"https://github.com/acme/widget","runner_event":"register"}"#;
    RequestSummary::new(
        &method,
        &target,
        authorization_scheme,
        credential_matches,
        content_length,
        expected_admin_body,
    )
}

fn classify_authorization(
    authorization: Option<&str>,
    expected_scheme: &str,
    expected_credential: &str,
) -> (&'static str, bool) {
    let Some((scheme, credential)) = authorization.and_then(|value| value.split_once(' ')) else {
        return ("unexpected", false);
    };
    let scheme_name = match scheme {
        "Bearer" => "Bearer",
        "RemoteAuth" => "RemoteAuth",
        _ => return ("unexpected", false),
    };
    (
        scheme_name,
        scheme == expected_scheme && credential == expected_credential,
    )
}

use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

use velnor_runner_github::{
    DiscoveryCredentialOutcome, DiscoveryCredentialStep, DiscoveryIntentId, DiscoveryIntentStore,
    DiscoveryTransport, Method, SessionError, SessionRequest, Transport, TransportFail,
    issue_repository_discovery_token, read_repository_admin_evidence,
};

use super::{BoundedDiscoveryTransport, Origin, actions_base, validate_discovery_request};

fn api_get(path: &str) -> SessionRequest {
    SessionRequest {
        method: Method::Get,
        path: path.to_owned(),
        query: None,
        headers: vec![
            (
                "Accept".to_owned(),
                "application/vnd.github+json".to_owned(),
            ),
            ("Authorization".to_owned(), "Bearer host-secret".to_owned()),
            ("X-GitHub-Api-Version".to_owned(), "2026-03-10".to_owned()),
            ("User-Agent".to_owned(), "velnor-host".to_owned()),
        ],
        body: Vec::new(),
    }
}

fn actions_get(path: &str, query: &str) -> SessionRequest {
    SessionRequest {
        method: Method::Get,
        path: path.to_owned(),
        query: Some(query.to_owned()),
        headers: vec![
            ("Content-Type".to_owned(), "application/json".to_owned()),
            ("Authorization".to_owned(), "Bearer admin-secret".to_owned()),
            ("User-Agent".to_owned(), "velnor-host".to_owned()),
        ],
        body: Vec::new(),
    }
}

#[test]
fn actions_service_origin_is_exact_regional_https_and_path_safe() {
    assert_eq!(
        actions_base("https://pipelinesghubeus13.actions.githubusercontent.com"),
        Some("https://pipelinesghubeus13.actions.githubusercontent.com".to_owned())
    );
    assert_eq!(
        actions_base("https://pipelinesghubeus13.actions.githubusercontent.com/"),
        Some("https://pipelinesghubeus13.actions.githubusercontent.com".to_owned())
    );
    assert_eq!(
        actions_base("https://pipelinesghubeus13.actions.githubusercontent.com/tenant-12/"),
        Some("https://pipelinesghubeus13.actions.githubusercontent.com/tenant-12".to_owned())
    );
    assert_eq!(
        actions_base("https://pipelinesghubeus13.actions.githubusercontent.com:443/tenant-12"),
        Some("https://pipelinesghubeus13.actions.githubusercontent.com/tenant-12".to_owned())
    );
    for rejected in [
        "http://pipelinesghubeus13.actions.githubusercontent.com",
        "https://pipelines.actions.githubusercontent.com",
        "https://pipelinesghubeus0.actions.githubusercontent.com",
        "https://pipelinesghubeus13.actions.githubusercontent.com.evil.test",
        "https://user@pipelinesghubeus13.actions.githubusercontent.com",
        "https://pipelinesghubeus13.actions.githubusercontent.com:444",
        "https://pipelinesghubeus13.actions.githubusercontent.com/?x=y",
        "https://pipelinesghubeus13.actions.githubusercontent.com/#fragment",
        "https://pipelinesghubeus13.actions.githubusercontent.com/%2e%2e/tenant",
        "https://pipelinesghubeus13.actions.githubusercontent.com//tenant",
        "https://pipelinesghubeus13.actions.githubusercontent.com/../tenant",
    ] {
        assert_eq!(actions_base(rejected), None, "accepted {rejected}");
    }
    let mut transport = BoundedDiscoveryTransport::new();
    assert_eq!(
        transport.bind_actions_service_origin("https://wrong.example"),
        Err(SessionError::Wire(
            velnor_runner_github::WireError::RegistrationRejected
        ))
    );
    assert!(!format!("{transport:?}").contains("wrong.example"));
}

#[test]
fn route_and_query_allowlist_exposes_only_discovery_gets_and_two_auth_posts() {
    let api = Origin::GithubApi;
    let actions =
        Origin::Actions("https://pipelinesghubeus13.actions.githubusercontent.com".to_owned());
    github_routes_are_allowlisted(&api);
    actions_routes_are_allowlisted(&actions);
    unsupported_routes_are_rejected(&api, &actions);
}

fn github_routes_are_allowlisted(api: &Origin) {
    assert!(validate_discovery_request(api, &api_get("repos/acme/widget")).is_some());
    assert!(
        validate_discovery_request(
            api,
            &SessionRequest {
                method: Method::Post,
                path: "/repos/acme/widget/actions/runners/registration-token".to_owned(),
                query: None,
                headers: vec![
                    (
                        "Content-Type".to_owned(),
                        "application/vnd.github.v3+json".to_owned()
                    ),
                    ("Authorization".to_owned(), "Bearer host-secret".to_owned()),
                    ("User-Agent".to_owned(), "velnor-host".to_owned()),
                ],
                body: Vec::new(),
            }
        )
        .is_some()
    );
    assert!(
        validate_discovery_request(
            api,
            &SessionRequest {
                method: Method::Post,
                path: "actions/runner-registration".to_owned(),
                query: None,
                headers: vec![
                    ("Content-Type".to_owned(), "application/json".to_owned()),
                    (
                        "Authorization".to_owned(),
                        "RemoteAuth registration-secret".to_owned()
                    ),
                    ("User-Agent".to_owned(), "velnor-host".to_owned()),
                ],
                body: br#"{"url":"https://github.com/acme/widget","runner_event":"register"}"#
                    .to_vec(),
            }
        )
        .is_some()
    );
}

fn actions_routes_are_allowlisted(actions: &Origin) {
    assert!(
        validate_discovery_request(
            actions,
            &actions_get("_apis/runtime/runnergroups", "api-version=6.0-preview")
        )
        .is_some()
    );
    assert!(
        validate_discovery_request(
            actions,
            &actions_get(
                "_apis/runtime/runnerscalesets",
                "api-version=6.0-preview&name=ubuntu-24.04-scale-set&runnerGroupId=19"
            )
        )
        .is_some()
    );
    for (path, query) in [
        (
            "_apis/runtime/runnerscalesets",
            "api-version=6.0-preview&name=ubuntu-24.04-scale-set&runnerGroupId=0",
        ),
        (
            "_apis/runtime/runnerscalesets",
            "api-version=6.0-preview&name=ubuntu-24.04-scale-set&runnerGroupId=019",
        ),
        (
            "_apis/runtime/runnerscalesets",
            "api-version=6.0-preview&name=ubuntu-24.04-scale-set&runnerGroupId=19&x=y",
        ),
        (
            "_apis/runtime/runnerscalesets",
            "api-version=6.0-preview&name=other&runnerGroupId=19",
        ),
        ("_apis/runtime/runnerscalesets/1", "api-version=6.0-preview"),
        ("_apis/runtime/runnergroups", "api-version=6.0-preview&x=y"),
    ] {
        assert!(validate_discovery_request(actions, &actions_get(path, query)).is_none());
    }
}

fn unsupported_routes_are_rejected(api: &Origin, actions: &Origin) {
    let mut create = actions_get(
        "_apis/runtime/runnerscalesets",
        "api-version=6.0-preview&name=ubuntu-24.04-scale-set&runnerGroupId=19",
    );
    create.method = Method::Post;
    assert!(validate_discovery_request(actions, &create).is_none());
    assert!(validate_discovery_request(api, &api_get("repos/acme/widget/actions/runs")).is_none());
    let mut extra_header = api_get("repos/acme/widget");
    extra_header
        .headers
        .push(("X-Forwarded-Host".to_owned(), "evil.test".to_owned()));
    assert!(validate_discovery_request(api, &extra_header).is_none());
}

#[test]
fn authentication_headers_are_singleton_exact_and_redacted_from_debug() {
    let request = api_get("repos/acme/widget");
    assert!(validate_discovery_request(&Origin::GithubApi, &request).is_some());
    let mut duplicate = request.clone();
    duplicate
        .headers
        .push(("authorization".to_owned(), "Bearer second".to_owned()));
    assert!(validate_discovery_request(&Origin::GithubApi, &duplicate).is_none());
    let mut transport = BoundedDiscoveryTransport::new();
    transport
        .bind_actions_service_origin(
            "https://pipelinesghubeus13.actions.githubusercontent.com/secret-route",
        )
        .expect("Actions origin should validate");
    let debug = format!("{transport:?} {request:?}");
    assert!(!debug.contains("admin-secret"));
    assert!(!debug.contains("host-secret"));
    assert!(!debug.contains("secret-route"));
}

#[test]
fn chunked_response_over_limit_is_rejected_before_it_reaches_json_decoder() {
    let (base, server) = chunked_server(96 * 1024, Duration::ZERO);
    let mut transport = BoundedDiscoveryTransport::for_test(base, 4096, Duration::from_secs(3));
    transport
        .bind_github_api_origin()
        .expect("GitHub origin should bind");
    let started = Instant::now();
    let result = transport.exchange(&api_get("repos/acme/widget"));
    assert_eq!(result, Err(TransportFail::Reset));
    assert!(started.elapsed() < Duration::from_secs(2));
    server.join().expect("server should complete");
}

#[test]
fn a_slow_chunked_response_obeys_one_whole_request_deadline() {
    let (base, server) = chunked_server(32 * 1024, Duration::from_millis(80));
    let mut transport =
        BoundedDiscoveryTransport::for_test(base, 64 * 1024, Duration::from_millis(350));
    transport
        .bind_github_api_origin()
        .expect("GitHub origin should bind");
    let started = Instant::now();
    assert_eq!(
        transport.exchange(&api_get("repos/acme/widget")),
        Err(TransportFail::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    let _joined = server.join();
}

#[test]
fn redirects_are_returned_without_following_the_location() {
    let target = TcpListener::bind("127.0.0.1:0").expect("target listener should bind");
    target
        .set_nonblocking(true)
        .expect("target listener should be nonblocking");
    let target_addr = target.local_addr().expect("target address should resolve");
    let (base, server) = one_response_server(
        format!("HTTP/1.1 302 Found\r\nLocation: http://{target_addr}/exfiltrate\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes(),
        Duration::from_millis(100),
    );
    let mut transport = BoundedDiscoveryTransport::for_test(base, 1024, Duration::from_secs(2));
    transport
        .bind_github_api_origin()
        .expect("GitHub origin should bind");
    let response = transport
        .exchange(&api_get("repos/acme/widget"))
        .expect("bounded request should complete");
    assert_eq!(response.status, 302);
    let mut accepted = false;
    let deadline = Instant::now() + Duration::from_millis(150);
    while Instant::now() < deadline {
        match target.accept() {
            Ok(_) => accepted = true,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("redirect probe accept failed: {error}"),
        }
    }
    assert!(!accepted, "transport followed an untrusted Location");
    server.join().expect("server should complete");
}

#[test]
fn failed_durable_intent_stops_before_the_registration_token_post() {
    let response =
        br#"{"id":22,"full_name":"acme/widget","private":true,"permissions":{"admin":true}}"#;
    let (base, server) =
        one_response_server(http_response(200, response), Duration::from_millis(250));
    let mut transport = BoundedDiscoveryTransport::for_test(base, 4096, Duration::from_secs(2));
    let evidence = read_repository_admin_evidence(&mut transport, "acme", "widget", "host-secret")
        .expect("repository read should complete");
    assert_eq!(evidence.repository_id(), 22);
    assert!(
        issue_repository_discovery_token(
            &mut transport,
            evidence,
            "host-secret",
            &mut RejectIntent
        )
        .is_err()
    );
    assert_eq!(
        server.join().expect("server should complete"),
        1,
        "credential POST escaped the failed intent gate"
    );
}

struct RejectIntent;

impl DiscoveryIntentStore for RejectIntent {
    fn persist_before(
        &mut self,
        _step: DiscoveryCredentialStep,
        _repository_id: i64,
        _full_name: &str,
    ) -> Result<DiscoveryIntentId, SessionError> {
        Err(SessionError::Uncertain)
    }

    fn record_outcome(
        &mut self,
        _id: DiscoveryIntentId,
        _outcome: DiscoveryCredentialOutcome,
    ) -> Result<(), SessionError> {
        panic!("no credential request means no outcome callback")
    }
}

#[path = "discovery_test_support.rs"]
mod support;
pub(super) use support::{chunked_server, http_response, one_response_server, read_headers};
#[path = "discovery_actions_read_tests.rs"]
mod actions_read_tests;
#[path = "discovery_async_tests.rs"]
mod async_tests;
#[path = "discovery_flow_tests.rs"]
mod flow;

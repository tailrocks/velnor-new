use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use velnor_runner_github::{
    BearerRole, DiscoveryTransport, Method, RequestPurpose, SessionRequest, Transport,
};

use super::{BoundedDiscoveryTransport, Origin, validate_discovery_request};

fn actions_delete(path: &str, query: &str) -> SessionRequest {
    SessionRequest {
        purpose: RequestPurpose::SessionClose,
        bearer_role: BearerRole::ActionsAdmin,
        method: Method::Delete,
        path: path.to_owned(),
        query: Some(query.to_owned()),
        headers: vec![
            ("Content-Type".to_owned(), "application/json".to_owned()),
            ("Authorization".to_owned(), "Bearer admin-secret".to_owned()),
        ],
        body: Vec::new(),
    }
}

fn actions_origin() -> Origin {
    Origin::Actions("https://pipelinesghubeus13.actions.githubusercontent.com".to_owned())
}

#[test]
fn exact_session_delete_route_and_query_are_allowlisted() {
    let actions = actions_origin();
    let request = actions_delete(
        "/_apis/runtime/runnerscalesets/42/sessions/session-123_X",
        "api-version=6.0-preview",
    );
    let target = validate_discovery_request(&actions, &request).expect("exact route");
    assert_eq!(
        target.path,
        "_apis/runtime/runnerscalesets/42/sessions/session-123_X"
    );
    assert_eq!(target.query, Some("api-version=6.0-preview"));
    let at_limit = format!(
        "_apis/runtime/runnerscalesets/42/sessions/{}",
        "s".repeat(256)
    );
    assert!(
        validate_discovery_request(
            &actions,
            &actions_delete(&at_limit, "api-version=6.0-preview")
        )
        .is_some()
    );
    let tilde = actions_delete(
        "_apis/runtime/runnerscalesets/42/sessions/session~1",
        "api-version=6.0-preview",
    );
    assert!(validate_discovery_request(&actions, &tilde).is_some());
}

#[test]
fn invalid_session_delete_routes_and_queries_are_rejected() {
    let actions = actions_origin();
    for (path, query) in [
        (
            "_apis/runtime/runnerscalesets/0/sessions/session-1",
            "api-version=6.0-preview",
        ),
        (
            "_apis/runtime/runnerscalesets/01/sessions/session-1",
            "api-version=6.0-preview",
        ),
        (
            "_apis/runtime/runnerscalesets/9223372036854775808/sessions/session-1",
            "api-version=6.0-preview",
        ),
        (
            "_apis/runtime/runnerscalesets/42/sessions/session-1/extra",
            "api-version=6.0-preview",
        ),
        (
            "_apis/runtime/runnerscalesets/42/sessions/",
            "api-version=6.0-preview",
        ),
        (
            "_apis/runtime/runnerscalesets/42/sessions/..",
            "api-version=6.0-preview",
        ),
        (
            &format!(
                "_apis/runtime/runnerscalesets/42/sessions/{}",
                "s".repeat(257)
            ),
            "api-version=6.0-preview",
        ),
        (
            "_apis/runtime/runnerscalesets/42/sessions/session/1",
            "api-version=6.0-preview",
        ),
        (
            "_apis/runtime/runnerscalesets/42/sessions/session\\1",
            "api-version=6.0-preview",
        ),
        (
            "_apis/runtime/runnerscalesets/42/sessions/session%2f1",
            "api-version=6.0-preview",
        ),
        (
            "_apis/runtime/runnerscalesets/42/sessions/session-1",
            "api-version=6.0-preview&x=1",
        ),
        (
            "_apis/runtime/runnerscalesets/42/sessions/session-1",
            "api-version=6.0-preview&api-version=6.0-preview",
        ),
        (
            "_apis/runtime/runnerscalesets/42/sessions/session-1",
            "api-version=7.0",
        ),
    ] {
        assert!(
            validate_discovery_request(&actions, &actions_delete(path, query)).is_none(),
            "unexpectedly accepted {path}?{query}"
        );
    }
}

#[test]
fn session_delete_requires_empty_body_and_exact_admin_headers() {
    let actions = actions_origin();
    let valid = actions_delete(
        "_apis/runtime/runnerscalesets/42/sessions/session-1",
        "api-version=6.0-preview",
    );
    let mut body = valid.clone();
    body.body = b"{}".to_vec();
    assert!(validate_discovery_request(&actions, &body).is_none());

    let mut wrong_method = valid.clone();
    wrong_method.method = Method::Patch;
    assert!(validate_discovery_request(&actions, &wrong_method).is_none());
    let mut wrong_auth = valid.clone();
    wrong_auth.headers[1].1 = "Bearer".to_owned();
    assert!(validate_discovery_request(&actions, &wrong_auth).is_none());
    let mut wrong_content_type = valid.clone();
    wrong_content_type.headers[0].1 = "application/json; charset=utf-8".to_owned();
    assert!(validate_discovery_request(&actions, &wrong_content_type).is_none());
    let mut extra_header = valid.clone();
    extra_header
        .headers
        .push(("Accept".to_owned(), "application/json".to_owned()));
    assert!(validate_discovery_request(&actions, &extra_header).is_none());
    let mut unexpected_user_agent = valid.clone();
    unexpected_user_agent
        .headers
        .push(("User-Agent".to_owned(), "velnor-host".to_owned()));
    assert!(validate_discovery_request(&actions, &unexpected_user_agent).is_none());
    let mut duplicate_auth = valid.clone();
    duplicate_auth
        .headers
        .push(("authorization".to_owned(), "Bearer second".to_owned()));
    assert!(validate_discovery_request(&actions, &duplicate_auth).is_none());

    let get = SessionRequest {
        purpose: RequestPurpose::ActionsMetadataRead,
        bearer_role: BearerRole::ActionsAdmin,
        method: Method::Get,
        path: "_apis/runtime/runnergroups".to_owned(),
        query: Some("api-version=6.0-preview".to_owned()),
        headers: vec![
            ("Content-Type".to_owned(), "application/json".to_owned()),
            ("Authorization".to_owned(), "Bearer admin-secret".to_owned()),
            ("User-Agent".to_owned(), "velnor-host".to_owned()),
        ],
        body: Vec::new(),
    };
    assert!(validate_discovery_request(&actions, &get).is_some());
}

struct RequestObservation {
    method: String,
    target: String,
    headers: HeaderObservation,
    body: BodyObservation,
    extra_requests: usize,
}

struct HeaderObservation {
    admin_bearer: bool,
    json_content_type: bool,
    user_agent: bool,
}

struct BodyObservation {
    empty: bool,
    no_transfer_encoding: bool,
}

fn accept_request(listener: &TcpListener) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "DELETE request did not arrive");
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("fake listener failed: {error}"),
        }
    }
}

fn request_headers(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("header timeout");
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    while bytes.len() < 16 * 1024 {
        if stream.read_exact(&mut byte).is_err() {
            break;
        }
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    bytes
}

fn observe_headers(headers: &[u8]) -> (String, String, HeaderObservation, bool, bool) {
    let text = std::str::from_utf8(headers).expect("request headers should be ASCII");
    let mut fields = text.lines().next().unwrap_or("").split_whitespace();
    let method = fields.next().unwrap_or("").to_owned();
    let target = fields.next().unwrap_or("").to_owned();
    let has_header = |name: &str, value: &str| {
        text.lines().any(|line| {
            line.split_once(':').is_some_and(|(key, actual)| {
                key.eq_ignore_ascii_case(name) && actual.trim() == value
            })
        })
    };
    let content_length_zero = text.lines().all(|line| {
        line.split_once(':').is_none_or(|(key, value)| {
            !key.eq_ignore_ascii_case("content-length") || value.trim() == "0"
        })
    });
    let no_transfer_encoding = !text.lines().any(|line| {
        line.split_once(':')
            .is_some_and(|(key, _)| key.eq_ignore_ascii_case("transfer-encoding"))
    });
    (
        method,
        target,
        HeaderObservation {
            admin_bearer: has_header("authorization", "Bearer admin-secret"),
            json_content_type: has_header("content-type", "application/json"),
            user_agent: has_header("user-agent", "velnor-host"),
        },
        content_length_zero,
        no_transfer_encoding,
    )
}

fn observe_body_and_retries(stream: &mut TcpStream, listener: &TcpListener) -> (bool, usize) {
    stream
        .set_read_timeout(Some(Duration::from_millis(40)))
        .expect("body timeout");
    let mut byte = [0u8; 1];
    let body_count = match stream.read(&mut byte) {
        Ok(count) => count,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            0
        }
        Err(error) => panic!("request body read failed: {error}"),
    };
    stream
        .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .expect("write 204");
    let deadline = Instant::now() + Duration::from_millis(300);
    let mut extra_requests = 0;
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut extra, _)) => {
                extra_requests += 1;
                let _ = request_headers(&mut extra);
                extra.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").expect("write retry response");
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("retry listener failed: {error}"),
        }
    }
    (body_count == 0, extra_requests)
}

fn serve_delete_once(listener: &TcpListener) -> RequestObservation {
    let mut stream = accept_request(listener);
    let headers = request_headers(&mut stream);
    let observed = observe_headers(&headers);
    let (empty_body, extra_requests) = observe_body_and_retries(&mut stream, listener);
    RequestObservation {
        method: observed.0,
        target: observed.1,
        headers: observed.2,
        body: BodyObservation {
            empty: empty_body && observed.3,
            no_transfer_encoding: observed.4,
        },
        extra_requests,
    }
}

#[test]
fn exact_session_delete_is_dispatched_once_to_bound_actions_origin() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fake listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should be nonblocking");
    let base = format!(
        "http://{}",
        listener.local_addr().expect("listener address")
    );
    let server = thread::spawn(move || serve_delete_once(&listener));

    let mut transport = BoundedDiscoveryTransport::for_test(base, 4096, Duration::from_secs(2));
    transport
        .bind_actions_service_origin(
            "https://pipelinesghubeus13.actions.githubusercontent.com/tenant",
        )
        .expect("fixed Actions Service origin should bind");
    let request = actions_delete(
        "_apis/runtime/runnerscalesets/42/sessions/session-123_X",
        "api-version=6.0-preview",
    );
    let response = transport
        .exchange(&request)
        .expect("DELETE should dispatch");
    assert_eq!(response.status, 204);
    assert_eq!(response.body, Vec::<u8>::new());

    let observed = server.join().expect("fake server should finish");
    assert_eq!(observed.method, "DELETE");
    assert_eq!(
        observed.target,
        "/tenant/_apis/runtime/runnerscalesets/42/sessions/session-123_X?api-version=6.0-preview"
    );
    assert!(observed.headers.admin_bearer);
    assert!(observed.headers.json_content_type);
    assert!(!observed.headers.user_agent);
    assert!(observed.body.empty);
    assert!(observed.body.no_transfer_encoding);
    assert_eq!(observed.extra_requests, 0);
}

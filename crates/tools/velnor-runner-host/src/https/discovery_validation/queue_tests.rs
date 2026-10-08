use velnor_runner_github::{
    BearerRole, DiscoveryTransport, Method, RequestPurpose, SessionRequest,
};

use super::super::super::{BoundedDiscoveryTransport, validate_discovery_request};

const QUEUE_URL: &str = "https://pipelinesghubeus13.actions.githubusercontent.com/_apis/runtime/messages?z=two+words&lastMessageId=3&a=%2B&lastMessageId=4";
const QUEUE_PATH: &str = "/_apis/runtime/messages";
const RAW_QUERY: &str = "z=two+words&lastMessageId=3&a=%2B&lastMessageId=4";
const CANONICAL_CURSOR_QUERY: &str = "a=%2B&lastMessageId=9&z=two+words";

fn bound_queue() -> BoundedDiscoveryTransport {
    let mut transport = BoundedDiscoveryTransport::new();
    let returned_route = transport
        .bind_message_queue_origin(QUEUE_URL)
        .expect("valid queue URL should bind");
    assert!(returned_route.matches_poll_target(QUEUE_PATH, Some(RAW_QUERY)));
    transport
}

fn poll(path: &str, query: Option<&str>) -> SessionRequest {
    SessionRequest {
        purpose: RequestPurpose::MessageQueuePoll,
        bearer_role: BearerRole::SessionQueue,
        method: Method::Get,
        path: path.to_owned(),
        query: query.map(str::to_owned),
        headers: vec![
            (
                "Accept".to_owned(),
                "application/json; api-version=6.0-preview".to_owned(),
            ),
            ("Authorization".to_owned(), "Bearer queue-marker".to_owned()),
            ("User-Agent".to_owned(), "velnor-host".to_owned()),
            ("X-ScaleSetMaxCapacity".to_owned(), "12".to_owned()),
        ],
        body: Vec::new(),
    }
}

fn ack(path: &str, query: Option<&str>) -> SessionRequest {
    SessionRequest {
        purpose: RequestPurpose::MessageAcknowledge,
        bearer_role: BearerRole::SessionQueue,
        method: Method::Delete,
        path: path.to_owned(),
        query: query.map(str::to_owned),
        headers: vec![
            ("Content-Type".to_owned(), "application/json".to_owned()),
            ("Authorization".to_owned(), "Bearer queue-marker".to_owned()),
            ("User-Agent".to_owned(), "velnor-host".to_owned()),
        ],
        body: Vec::new(),
    }
}

#[test]
fn bound_queue_accepts_exact_poll_and_ack_targets_with_preserved_queries() {
    let transport = bound_queue();
    let origin = transport.origin.as_ref().expect("bound queue origin");

    let raw_poll = poll(QUEUE_PATH, Some(RAW_QUERY));
    let target = validate_discovery_request(origin, &raw_poll).expect("initial poll target");
    assert_eq!(target.path, "_apis/runtime/messages");
    assert_eq!(target.query, Some(RAW_QUERY));

    let cursor_poll = poll(QUEUE_PATH, Some(CANONICAL_CURSOR_QUERY));
    let target = validate_discovery_request(origin, &cursor_poll).expect("cursor poll target");
    assert_eq!(target.path, "_apis/runtime/messages");
    assert_eq!(target.query, Some(CANONICAL_CURSOR_QUERY));

    let acknowledge = ack("/_apis/runtime/messages/17", Some(RAW_QUERY));
    let target = validate_discovery_request(origin, &acknowledge).expect("ack target");
    assert_eq!(target.path, "_apis/runtime/messages/17");
    assert_eq!(target.query, Some(RAW_QUERY));
}

#[test]
fn queue_route_rejects_wrong_origin_path_query_or_request_role() {
    let transport = bound_queue();
    let origin = transport.origin.as_ref().expect("bound queue origin");
    for request in [
        poll("/_apis/runtime/other", Some(RAW_QUERY)),
        poll(QUEUE_PATH, Some("lastMessageId=9&a=%2B&z=two+words")),
        poll(
            QUEUE_PATH,
            Some("a=%2B&lastMessageId=9&z=two+words&extra=1"),
        ),
        poll("//_apis/runtime/messages", Some(RAW_QUERY)),
        ack("/_apis/runtime/messages/017", Some(RAW_QUERY)),
        ack("/_apis/runtime/messages/17", Some("changed=1")),
    ] {
        assert!(
            validate_discovery_request(origin, &request).is_none(),
            "queue route unexpectedly accepted a malformed target"
        );
    }

    let mut wrong_purpose = poll(QUEUE_PATH, Some(RAW_QUERY));
    wrong_purpose.purpose = RequestPurpose::AcquireJobs;
    assert!(validate_discovery_request(origin, &wrong_purpose).is_none());
    let mut wrong_role = poll(QUEUE_PATH, Some(RAW_QUERY));
    wrong_role.bearer_role = BearerRole::ActionsAdmin;
    assert!(validate_discovery_request(origin, &wrong_role).is_none());
    let mut wrong_method = ack("/_apis/runtime/messages/17", Some(RAW_QUERY));
    wrong_method.method = Method::Post;
    assert!(validate_discovery_request(origin, &wrong_method).is_none());
}

#[test]
fn queue_poll_requires_exact_nonsecret_header_shape_and_empty_body() {
    let transport = bound_queue();
    let origin = transport.origin.as_ref().expect("bound queue origin");
    let valid = poll(QUEUE_PATH, Some(RAW_QUERY));
    let mut duplicate = valid.clone();
    duplicate
        .headers
        .push(("authorization".to_owned(), "Bearer another".to_owned()));
    assert!(validate_discovery_request(origin, &duplicate).is_none());
    let mut extra = valid.clone();
    extra.headers.push(("X-Extra".to_owned(), "no".to_owned()));
    assert!(validate_discovery_request(origin, &extra).is_none());
    let mut wrong_accept = valid.clone();
    wrong_accept.headers[0].1 = "application/json".to_owned();
    assert!(validate_discovery_request(origin, &wrong_accept).is_none());
    let mut bad_capacity = valid.clone();
    bad_capacity.headers[3].1 = "012".to_owned();
    assert!(validate_discovery_request(origin, &bad_capacity).is_none());
    bad_capacity.headers[3].1 = "4294967296".to_owned();
    assert!(validate_discovery_request(origin, &bad_capacity).is_none());
    let mut body = valid;
    body.body = b"{}".to_vec();
    assert!(validate_discovery_request(origin, &body).is_none());
}

#[test]
fn queue_ack_requires_exact_admin_free_headers_and_empty_body() {
    let transport = bound_queue();
    let origin = transport.origin.as_ref().expect("bound queue origin");
    let valid = ack("/_apis/runtime/messages/17", Some(RAW_QUERY));
    let mut extra = valid.clone();
    extra
        .headers
        .push(("Accept".to_owned(), "application/json".to_owned()));
    assert!(validate_discovery_request(origin, &extra).is_none());
    let mut wrong_auth = valid.clone();
    wrong_auth.headers[1].1 = "Bearer".to_owned();
    assert!(validate_discovery_request(origin, &wrong_auth).is_none());
    let mut body = valid;
    body.body = b"{}".to_vec();
    assert!(validate_discovery_request(origin, &body).is_none());
}

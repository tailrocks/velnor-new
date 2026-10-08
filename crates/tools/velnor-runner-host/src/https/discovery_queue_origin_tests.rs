use velnor_runner_github::{
    BearerRole, DiscoveryTransport, MessageQueueRoute, Method, RequestPurpose, SessionRequest,
};

use super::{
    BoundedDiscoveryTransport, Origin, queue_origin::queue_route, validate_discovery_request,
};

#[test]
fn queue_origin_binding_is_distinct_and_hides_the_server_returned_route() {
    let url = "https://pipelinesghubeus13.actions.githubusercontent.com/_apis/runtime/messages?opaque=secret%2Froute";
    let mut transport = BoundedDiscoveryTransport::new();
    let route = transport
        .bind_message_queue_origin(url)
        .expect("documented Actions queue origin should bind separately");
    assert!(!format!("{transport:?}").contains("pipelinesghubeus13"));
    assert!(!format!("{route:?}").contains("opaque=secret"));
    assert!(matches!(
        transport.origin.as_ref(),
        Some(Origin::MessageQueue(queue_origin))
            if queue_origin.base == "https://pipelinesghubeus13.actions.githubusercontent.com"
    ));

    let parts = queue_route(url).expect("valid queue URL");
    assert_eq!(parts.path, "/_apis/runtime/messages");
    assert_eq!(parts.query.as_deref(), Some("opaque=secret%2Froute"));
    assert_eq!(parts.path, "/_apis/runtime/messages");
    assert_eq!(parts.query.as_deref(), Some("opaque=secret%2Froute"));
    let target = transport
        .request_url(&SessionRequest {
            purpose: RequestPurpose::MessageQueuePoll,
            bearer_role: BearerRole::SessionQueue,
            method: Method::Get,
            path: "/_apis/runtime/messages".to_owned(),
            query: Some("opaque=secret%2Froute".to_owned()),
            headers: vec![
                (
                    "Accept".to_owned(),
                    "application/json; api-version=6.0-preview".to_owned(),
                ),
                ("Authorization".to_owned(), "Bearer queue-secret".to_owned()),
                ("User-Agent".to_owned(), "velnor-host".to_owned()),
                ("X-ScaleSetMaxCapacity".to_owned(), "1".to_owned()),
            ],
            body: Vec::new(),
        })
        .expect("bound route should produce its exact URL");
    assert!(target.ends_with("/_apis/runtime/messages?opaque=secret%2Froute"));
}

#[test]
fn queue_binding_rejects_unsafe_path_or_query_before_returning_a_route() {
    for url in [
        "https://pipelinesghubeus13.actions.githubusercontent.com//messages",
        "https://pipelinesghubeus13.actions.githubusercontent.com/a/../messages",
        "https://pipelinesghubeus13.actions.githubusercontent.com/%2fmessages",
        "https://pipelinesghubeus13.actions.githubusercontent.com/messages?bad=%Q0",
        "https://pipelinesghubeus13.actions.githubusercontent.com/messages?bad=a;b",
    ] {
        let parts = queue_route(url).expect("valid HTTPS origin and URL envelope");
        assert!(
            MessageQueueRoute::from_parts(parts.path.clone(), parts.query.clone()).is_err(),
            "unsafe queue route was accepted: {url}"
        );
    }
}

#[test]
fn invalid_queue_rebind_clears_the_previous_origin() {
    let mut transport = BoundedDiscoveryTransport::new();
    let route = transport
        .bind_message_queue_origin(
            "https://pipelinesghubeus13.actions.githubusercontent.com/messages?opaque=secret",
        )
        .expect("valid queue URL");
    assert!(!format!("{route:?}").contains("opaque=secret"));
    assert!(
        transport
            .bind_message_queue_origin("https://queue.attacker.invalid/messages")
            .is_err()
    );
    assert!(transport.origin.is_none());
}

#[test]
fn a_valid_github_path_rejects_mismatched_purpose_or_bearer_role() {
    let api = Origin::GithubApi;
    let mut request = SessionRequest {
        purpose: RequestPurpose::RepositoryRead,
        bearer_role: BearerRole::GithubRestCredential,
        method: Method::Get,
        path: "repos/acme/widget".to_owned(),
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
    };
    assert!(validate_discovery_request(&api, &request).is_some());
    request.bearer_role = BearerRole::ActionsAdmin;
    assert!(validate_discovery_request(&api, &request).is_none());
    request.bearer_role = BearerRole::GithubRestCredential;
    request.purpose = RequestPurpose::ActionsRead;
    assert!(validate_discovery_request(&api, &request).is_none());
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn test_transport_origins_are_loopback_only() {
    assert_eq!(
        super::parse_test_loopback_origin("http://127.0.0.1:8080")
            .ok()
            .as_deref(),
        Some("http://127.0.0.1:8080")
    );
    for invalid in [
        "https://api.github.com",
        "http://localhost:80",
        "http://127.0.0.1:0",
        "http://127.0.0.1:80/path",
        "http://127.0.0.1:80@evil.example",
        "http://127.0.0.2:80",
    ] {
        assert!(super::parse_test_loopback_origin(invalid).is_err());
    }
}
#[test]
fn conditional_reads_revalidate_before_reusing_response() {
    let body = r#"{"generation":1}"#;
    let first = format!(
        "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let unchanged = "HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned();
    let (origin, worker) = server(vec![first, unchanged]);
    let reader = client(origin);
    let route = "repos/example/demo-crate";
    assert_eq!(reader.get(route).expect("initial")["generation"], 1);
    assert_eq!(reader.get(route).expect("revalidated")["generation"], 1);
    let requests = worker.join().expect("server");
    assert!(!requests[0].to_ascii_lowercase().contains("if-none-match:"));
    assert!(
        requests[1]
            .to_ascii_lowercase()
            .contains("if-none-match: \"v1\"")
    );
}

fn tagged_reply(status: &str, tag: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nETag: {tag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn conditional_reads_never_hide_changes_or_failed_access() {
    let (origin, worker) = server(vec![
        tagged_reply("200 OK", "\"v1\"", r#"{"value":1}"#),
        tagged_reply("200 OK", "\"v2\"", r#"{"value":2}"#),
        reply("403 Forbidden", "{}"),
        tagged_reply("304 Not Modified", "\"v2\"", ""),
    ]);
    let reader = client(origin);
    let route = "repos/example/demo-crate";
    assert_eq!(reader.get(route).expect("first")["value"], 1);
    assert_eq!(reader.get(route).expect("changed")["value"], 2);
    assert!(reader.get(route).is_err());
    assert!(reader.get(route).is_err());
    let requests = worker.join().expect("server");
    assert!(
        requests[1]
            .to_ascii_lowercase()
            .contains("if-none-match: \"v1\"")
    );
    assert!(
        requests[2]
            .to_ascii_lowercase()
            .contains("if-none-match: \"v2\"")
    );
    assert!(!requests[3].to_ascii_lowercase().contains("if-none-match:"));
    assert!(requests.iter().all(|request| {
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer ")
    }));
}

#[test]
fn conditional_reads_require_matching_route_client_and_size() {
    let (origin, worker) = server(vec![
        tagged_reply("200 OK", "\"v1\"", r#"{"value":1}"#),
        tagged_reply("304 Not Modified", "\"v1\"", ""),
        tagged_reply("304 Not Modified", "\"v1\"", ""),
        tagged_reply("304 Not Modified", "\"v1\"", ""),
    ]);
    let reader = client(origin.clone());
    let route = "repos/example/demo-crate";
    assert!(reader.get(route).is_ok());
    assert!(reader.get("repos/example/another").is_err());
    assert!(reader.request_with_limit(route, false, 1024).is_err());
    assert!(client(origin).get(route).is_err());
    let requests = worker.join().expect("server");
    assert!(
        requests
            .iter()
            .all(|request| !request.to_ascii_lowercase().contains("if-none-match:"))
    );
}

#[test]
fn conditional_reads_reject_mismatched_validators_and_link_metadata() {
    let route =
        "repos/example/demo-crate/git/matching-refs/tags/velnor-receipt%2F42%2F1%2Foperation-";
    let linked = "HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nLink: <https://example.invalid>; rel=next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned();
    let (origin, worker) = server(vec![
        tagged_reply("200 OK", "\"v1\"", "[]"),
        tagged_reply("304 Not Modified", "\"v2\"", ""),
        tagged_reply("200 OK", "\"v1\"", "[]"),
        linked,
    ]);
    let reader = client(origin);
    assert!(reader.get(route).is_ok());
    assert!(reader.get(route).is_err());
    assert!(reader.get(route).is_ok());
    assert!(reader.get(route).is_err());
    worker.join().expect("server");
}

#[test]
fn conditional_reads_do_not_cache_no_store_or_missing_responses() {
    let no_store = "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\nCache-Control: no-store\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_owned();
    let (origin, worker) = server(vec![
        no_store,
        reply("200 OK", "{}"),
        tagged_reply("200 OK", "\"v1\"", "{}"),
        reply("404 Not Found", "{}"),
        tagged_reply("304 Not Modified", "\"v1\"", ""),
    ]);
    let reader = client(origin);
    let route = "repos/example/demo-crate/git/ref/tags/missing";
    assert!(reader.request_with_limit(route, true, 1024).is_ok());
    assert!(reader.request_with_limit(route, true, 1024).is_ok());
    assert!(reader.request_with_limit(route, true, 1024).is_ok());
    assert!(
        reader
            .request_with_limit(route, true, 1024)
            .expect("404")
            .is_none()
    );
    assert!(reader.request_with_limit(route, true, 1024).is_err());
    let requests = worker.join().expect("server");
    assert!(!requests[1].to_ascii_lowercase().contains("if-none-match:"));
    assert!(!requests[2].to_ascii_lowercase().contains("if-none-match:"));
    assert!(
        requests[3]
            .to_ascii_lowercase()
            .contains("if-none-match: \"v1\"")
    );
    assert!(!requests[4].to_ascii_lowercase().contains("if-none-match:"));
}

#[test]
fn conditional_reads_bound_response_storage() {
    let replies = (0..66)
        .map(|_| tagged_reply("200 OK", "\"v1\"", "{}"))
        .collect();
    let (origin, worker) = server(replies);
    let reader = client(origin);
    for index in 0..65 {
        assert!(reader.get(&format!("repos/example/crate-{index}")).is_ok());
    }
    assert!(reader.get("repos/example/crate-64").is_ok());
    assert_eq!(
        reader.responses.borrow().len(),
        super::MAX_VALIDATED_RESPONSES
    );
    let requests = worker.join().expect("server");
    assert!(!requests[65].to_ascii_lowercase().contains("if-none-match:"));
}

//! Curl argv and URL joins. These tests do not open a socket.

use velnor_runner_github::{Method, SessionRequest};

use super::https::{CURL_ARGV, HttpsTransport, join_url, trace_lines};
use crate::error::HostError;

#[test]
fn https_base_rejects_plain_http() {
    assert_eq!(
        HttpsTransport::new("http://api.github.com").map(|_| ()),
        Err(HostError::Endpoint)
    );
    assert!(HttpsTransport::new("https://api.github.com").is_ok());
}

#[test]
fn background_cleanup_client_has_a_short_deadline() {
    assert_eq!(
        HttpsTransport::new("https://api.github.com").map(|transport| transport.timeout_seconds()),
        Ok(60)
    );
    assert_eq!(
        HttpsTransport::new("https://api.github.com")
            .map(|transport| transport.cleanup_client().timeout_seconds()),
        Ok(5)
    );
}

#[test]
fn join_keeps_the_encoded_query() {
    let url = join_url(
        "https://api.github.com",
        "_apis/runtime/runnerscalesets",
        Some("api-version=6.0-preview&name=ubuntu-26.04-scale-set&runnerGroupId=1"),
    );
    assert_eq!(
        url.as_deref(),
        Some(
            "https://api.github.com/_apis/runtime/runnerscalesets?api-version=6.0-preview&name=ubuntu-26.04-scale-set&runnerGroupId=1"
        )
    );
}

#[test]
fn curl_argv_has_no_header() {
    let request = SessionRequest {
        method: Method::Post,
        path: "/repos/o/r/actions/runners/registration-token".to_owned(),
        query: None,
        headers: vec![("Authorization".to_owned(), "Bearer secret".to_owned())],
        body: Vec::new(),
    };
    let rendered = format!("{request:?}");
    assert!(rendered.contains("[redacted]"));
    assert!(!rendered.contains("secret"));
    assert!(!CURL_ARGV.iter().any(|arg| arg.contains("Authorization")));
}

#[test]
fn trace_preserves_success_and_empty_response_metadata() {
    let request = trace_request();
    assert_eq!(
        trace_lines("https://api.github.com", &request, 200, 12),
        ["trace host=api.github.com verb=GET path=/repos/o/r/actions/runners status=200 bytes=12"]
    );
    assert_eq!(
        trace_lines("https://api.github.com", &request, 204, 0),
        ["trace host=api.github.com verb=GET path=/repos/o/r/actions/runners status=204 bytes=0"]
    );
}

#[test]
fn trace_omits_non_jwt_and_jwt_error_body_fixtures() {
    let request = trace_request();
    let fixtures = [
        (
            401,
            r#"{"message":"invalid credential","access_token":"synthetic-pat-marker-not-a-secret"}"#,
        ),
        (
            503,
            r#"{"message":"expired credential","id_token":"eyJ_FAKE_HEADER.payload.signature"}"#,
        ),
    ];
    for (status, fixture) in fixtures {
        let lines = trace_lines("https://api.github.com", &request, status, fixture.len());
        let trace = lines.join("\n");
        assert!(trace.contains(&format!("status={status} bytes={}", fixture.len())));
        assert!(trace.contains("trace body=omitted"));
        assert!(!trace.contains(fixture));
        assert!(!trace.contains("synthetic-pat-marker-not-a-secret"));
        assert!(!trace.contains("eyJ_FAKE_HEADER"));
    }
}

fn trace_request() -> SessionRequest {
    SessionRequest {
        method: Method::Get,
        path: "/repos/o/r/actions/runners".to_owned(),
        query: None,
        headers: Vec::new(),
        body: Vec::new(),
    }
}

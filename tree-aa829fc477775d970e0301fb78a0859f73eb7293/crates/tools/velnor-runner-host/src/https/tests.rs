//! Curl argv and URL joins. These tests do not open a socket.

use velnor_runner_github::{BearerRole, Method, RequestPurpose, SessionRequest};

use super::{CURL_ARGV, HttpsTransport, join_url, trace_record};
use crate::HostError;

#[test]
fn https_base_rejects_plain_http() {
    assert_eq!(
        HttpsTransport::new("http://api.github.com").map(|_| ()),
        Err(HostError::Endpoint)
    );
    assert!(HttpsTransport::new("https://api.github.com").is_ok());
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
        purpose: RequestPurpose::RegistrationTokenIssue,
        bearer_role: BearerRole::GithubRestCredential,
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
fn trace_record_keeps_only_safe_http_diagnostics() {
    let request = SessionRequest {
        purpose: RequestPurpose::SessionClose,
        bearer_role: BearerRole::ActionsAdmin,
        method: Method::Delete,
        path: "/_apis/runtime/runnerscalesets/3/sessions/synthetic-path-marker".to_owned(),
        query: Some("synthetic-query-marker=never-log".to_owned()),
        headers: vec![(
            "Authorization".to_owned(),
            "Bearer synthetic-token-marker".to_owned(),
        )],
        body: b"synthetic-request-body-marker".to_vec(),
    };
    let response_body = b"synthetic-response-body-marker: private service diagnostic";

    let record = trace_record(&request, 403, response_body);

    assert_eq!(
        record,
        format!(
            "trace verb=DELETE status=403 class=client_error bytes={}",
            response_body.len()
        )
    );
    for marker in [
        "synthetic-path-marker",
        "synthetic-query-marker",
        "synthetic-token-marker",
        "synthetic-request-body-marker",
        "synthetic-response-body-marker",
        "private service diagnostic",
    ] {
        assert!(!record.contains(marker), "trace leaked marker: {marker}");
    }
}

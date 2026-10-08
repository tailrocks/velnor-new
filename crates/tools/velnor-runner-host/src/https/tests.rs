//! Curl argv and URL joins. These tests do not open a socket.

use std::fs;
use std::path::Path;

use velnor_runner_github::{BearerRole, Method, RequestPurpose, SessionRequest};

use super::{
    CURL_ARGV, CurlFail, HttpsTransport, MAX_RESPONSE_BYTES, Scratch, classify_exit, curl_config,
    join_url, perform_in_scratch, trace_record,
};
use crate::HostError;

fn sample_request() -> SessionRequest {
    SessionRequest {
        purpose: RequestPurpose::SessionClose,
        bearer_role: BearerRole::ActionsAdmin,
        method: Method::Post,
        path: "/_apis/runtime/runnerscalesets/3/sessions/close".to_owned(),
        query: None,
        headers: vec![("Authorization".to_owned(), "Bearer synthetic".to_owned())],
        body: b"synthetic-request".to_vec(),
    }
}

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

#[test]
fn curl_config_uses_shared_response_limit() {
    let request = sample_request();
    let config = curl_config(
        "https://pipelines.example.test/close",
        &request,
        Path::new("/tmp/body"),
        Path::new("/tmp/output"),
    )
    .expect("safe synthetic curl configuration");

    assert!(
        config
            .lines()
            .any(|line| { line == format!("max-filesize = {MAX_RESPONSE_BYTES}") })
    );
}

#[test]
fn bounded_response_preserves_bytes_and_removes_exact_scratch_directory() {
    let parent = tempfile::tempdir().expect("test parent");
    let scratch = Scratch::create_in(parent.path()).expect("private scratch");
    let scratch_path = scratch.dir.clone();
    let mut response = b"{\"jitConfig\":\"synthetic-jit-bytes\"}".to_vec();
    response.resize(MAX_RESPONSE_BYTES, b'j');
    let exchange = perform_in_scratch(
        "https://pipelines.example.test/session",
        &sample_request(),
        scratch,
        |_, output| {
            fs::write(output, &response).map_err(|_| CurlFail::Reset)?;
            Ok(200)
        },
    )
    .expect("bounded synthetic exchange");

    assert_eq!(exchange.status, 200);
    assert_eq!(exchange.body, response);
    assert!(!scratch_path.exists());
}

#[test]
fn oversized_response_is_rejected_and_scratch_is_removed() {
    let parent = tempfile::tempdir().expect("test parent");
    let scratch = Scratch::create_in(parent.path()).expect("private scratch");
    let scratch_path = scratch.dir.clone();
    let oversized = vec![b'x'; MAX_RESPONSE_BYTES + 1];
    let result = perform_in_scratch(
        "https://pipelines.example.test/session",
        &sample_request(),
        scratch,
        |_, output| {
            fs::write(output, &oversized).map_err(|_| CurlFail::Reset)?;
            Ok(200)
        },
    );

    assert!(matches!(result, Err(CurlFail::ResponseTooLarge)));
    assert!(!scratch_path.exists());
}

#[test]
fn curl_size_exit_is_not_an_empty_success_or_timeout() {
    assert!(matches!(
        classify_exit(Some(63), b"200"),
        Err(CurlFail::ResponseTooLarge)
    ));
}

#[test]
fn curl_failure_keeps_primary_classification_and_removes_scratch() {
    let parent = tempfile::tempdir().expect("test parent");
    let scratch = Scratch::create_in(parent.path()).expect("private scratch");
    let scratch_path = scratch.dir.clone();
    let result = perform_in_scratch(
        "https://pipelines.example.test/session",
        &sample_request(),
        scratch,
        |_, output| {
            fs::write(output, b"partial synthetic response").map_err(|_| CurlFail::Reset)?;
            Err(CurlFail::Timeout)
        },
    );

    assert!(matches!(result, Err(CurlFail::Timeout)));
    assert!(!scratch_path.exists());
}

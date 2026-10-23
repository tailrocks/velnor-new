use std::process::Command;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::{
    JSON_LIMIT, REDIRECT_LIMIT, agent_config, api_request_headers, checked_storage_uri, hex_lower,
    next_storage_uri, read_json_response, storage_request_headers, stream_archive,
    valid_archive_response_headers,
};

fn test_response(
    status: u16,
    headers: &[(&str, &str)],
    data: impl Into<Vec<u8>>,
) -> ureq::http::Response<ureq::Body> {
    let mut builder = ureq::http::Response::builder().status(status);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder
        .body(ureq::Body::builder().data(data))
        .expect("response")
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

#[test]
fn poisoned_proxy_environment_cannot_select_the_evidence_agent_proxy() {
    const CHILD_MARKER: &str = "VELNOR_EVIDENCE_PROXY_CONFIG_CHILD";
    if std::env::var_os(CHILD_MARKER).is_some() {
        let tls = ureq::tls::TlsConfig::builder().build();
        assert!(agent_config(tls).proxy().is_none());
        println!("proxy configuration probe completed");
        return;
    }

    let executable = std::env::current_exe().expect("test executable");
    let output = Command::new(executable)
        .args([
            "--exact",
            "api::github_evidence_http::tests::poisoned_proxy_environment_cannot_select_the_evidence_agent_proxy",
            "--nocapture",
        ])
        .env_clear()
        .env(CHILD_MARKER, "1")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .env("http_proxy", "http://127.0.0.1:9")
        .env("https_proxy", "http://127.0.0.1:9")
        .env("all_proxy", "http://127.0.0.1:9")
        .output()
        .expect("run isolated proxy configuration probe");
    assert!(
        output.status.success(),
        "proxy configuration child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("proxy configuration probe completed"),
        "proxy configuration child did not execute the exact test: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn artifact_redirects_are_https_and_limited_to_github_storage_hosts() {
    assert!(
        checked_storage_uri(
            "https://results.blob.core.windows.net/container/archive.zip?sig=redacted"
        )
        .is_ok()
    );
    assert!(
        checked_storage_uri("https://results.actions.githubusercontent.com/download/archive.zip")
            .is_ok()
    );
    for rejected in [
        "http://results.blob.core.windows.net/archive.zip",
        "https://user@example.blob.core.windows.net/archive.zip",
        "https://example.com/archive.zip",
        "https://blob.core.windows.net/archive.zip",
        "https://results.blob.core.windows.net:444/archive.zip",
    ] {
        assert!(
            checked_storage_uri(rejected).is_err(),
            "accepted {rejected}"
        );
    }
}

#[test]
fn api_authentication_headers_never_cross_to_storage_requests() {
    let api = api_request_headers("test-token");
    let storage = storage_request_headers();
    assert_eq!(
        api.iter()
            .find(|(name, _)| *name == "Authorization")
            .map(|(_, value)| value.as_str()),
        Some("Bearer test-token")
    );
    assert!(!api.iter().any(|(name, _)| *name == "Cookie"));
    assert!(
        !storage.iter().any(|(name, _)| {
            matches!(*name, "Authorization" | "Cookie" | "Proxy-Authorization")
        })
    );
    assert!(
        storage
            .iter()
            .any(|(name, value)| { *name == "Accept-Encoding" && value == "identity" })
    );
}

#[test]
fn storage_redirect_chain_is_bounded_and_revalidates_every_location() {
    for redirects in 0..REDIRECT_LIMIT {
        assert!(
            next_storage_uri(
                "https://results.blob.core.windows.net/container/archive.zip?sig=redacted",
                redirects
            )
            .is_ok()
        );
    }
    assert!(
        next_storage_uri(
            "https://results.blob.core.windows.net/container/archive.zip",
            REDIRECT_LIMIT
        )
        .is_err()
    );
    assert!(next_storage_uri("https://attacker.example/archive.zip", 0).is_err());
}

#[test]
fn json_response_status_body_bound_and_deadline_are_enforced_offline() {
    assert!(
        read_json_response(
            test_response(401, &[], b"unauthorized".to_vec()),
            deadline()
        )
        .is_err()
    );
    assert!(read_json_response(test_response(200, &[], Vec::new()), deadline()).is_err());
    assert!(
        read_json_response(
            test_response(
                200,
                &[],
                vec![b'x'; usize::try_from(JSON_LIMIT).expect("test limit fits") + 1],
            ),
            deadline()
        )
        .is_err()
    );
    assert!(
        read_json_response(
            test_response(200, &[], b"{}".to_vec()),
            Instant::now()
                .checked_sub(Duration::from_secs(1))
                .expect("past instant")
        )
        .is_err()
    );
    assert_eq!(
        read_json_response(test_response(200, &[], b"{}".to_vec()), deadline())
            .expect("valid JSON response"),
        b"{}"
    );
}

#[test]
fn archive_response_requires_identity_encoding_and_exact_optional_length() {
    use ureq::http::HeaderMap;

    let headers = |items: &[(&str, &str)]| {
        let mut headers = HeaderMap::new();
        for (name, value) in items {
            headers.append(
                ureq::http::HeaderName::from_bytes(name.as_bytes())
                    .expect("valid fixture header name"),
                ureq::http::HeaderValue::from_str(value).expect("valid fixture header value"),
            );
        }
        headers
    };
    assert!(valid_archive_response_headers(&headers(&[]), 4));
    assert!(valid_archive_response_headers(
        &headers(&[("content-length", "4"), ("content-encoding", "identity")]),
        4
    ));
    for invalid in [
        vec![("content-length", "3")],
        vec![("content-length", "wat")],
        vec![("content-length", "4"), ("content-length", "4")],
        vec![("content-encoding", "gzip")],
        vec![
            ("content-encoding", "identity"),
            ("content-encoding", "identity"),
        ],
    ] {
        assert!(!valid_archive_response_headers(&headers(&invalid), 4));
    }
}

#[test]
fn archive_stream_verifies_exact_bytes_digest_and_deadline() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let body = b"immutable archive bytes";
    let digest = hex_lower(&Sha256::digest(body));
    let path = dir.path().join("ok.zip");
    let mut response = test_response(200, &[], body.to_vec());
    let receipt = stream_archive(&mut response, &path, body.len() as u64, &digest, deadline())
        .expect("bounded archive stream");
    assert_eq!(receipt.size_bytes, body.len() as u64);
    assert_eq!(receipt.sha256, digest);
    assert_eq!(std::fs::read(path).expect("written archive"), body);

    let invalid_digest = "0".repeat(64);
    for (label, bytes, expected_size, expected_digest, until) in [
        (
            "truncated",
            b"short".as_slice(),
            6,
            digest.as_str(),
            deadline(),
        ),
        (
            "oversized",
            b"longer".as_slice(),
            3,
            digest.as_str(),
            deadline(),
        ),
        (
            "digest",
            body.as_slice(),
            body.len() as u64,
            invalid_digest.as_str(),
            deadline(),
        ),
        (
            "deadline",
            body.as_slice(),
            body.len() as u64,
            digest.as_str(),
            Instant::now()
                .checked_sub(Duration::from_secs(1))
                .expect("past instant"),
        ),
    ] {
        let path = dir.path().join(format!("{label}.zip"));
        let mut response = test_response(200, &[], bytes.to_vec());
        assert!(
            stream_archive(&mut response, &path, expected_size, expected_digest, until).is_err()
        );
        if path.exists() {
            std::fs::remove_file(path).expect("remove temporary stream output");
        }
    }
}

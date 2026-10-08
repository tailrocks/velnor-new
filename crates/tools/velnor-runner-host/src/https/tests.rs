//! Curl argv and URL joins. These tests do not open a socket.

use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use velnor_runner_github::{BearerRole, Method, RequestPurpose, SessionRequest};

use super::{
    BodyReadError, CURL_ARGV, CurlFail, HttpsTransport, MAX_RESPONSE_BYTES, Scratch, classify_exit,
    curl_config, join_url, perform_in_scratch, read_bounded, run_curl_with_executable,
    trace_record,
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
    )
    .expect("safe synthetic curl configuration");

    assert!(
        config
            .lines()
            .any(|line| { line == format!("max-filesize = {MAX_RESPONSE_BYTES}") })
    );
    assert!(config.lines().any(|line| line == "output = \"-\""));
}

#[test]
fn bounded_reader_stops_at_the_limit_plus_one_byte() {
    struct EndlessReader(usize);

    impl Read for EndlessReader {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.0 += bytes.len();
            bytes.fill(b'x');
            Ok(bytes.len())
        }
    }

    let mut reader = EndlessReader(0);
    assert!(matches!(
        read_bounded(&mut reader, 1024),
        Err(BodyReadError::TooLarge)
    ));
    assert_eq!(reader.0, 1025);
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
        |_| Ok((200, response.clone())),
    )
    .expect("bounded synthetic exchange");

    assert_eq!(exchange.status, 200);
    assert_eq!(exchange.body, response);
    assert!(!scratch_path.exists());
}

#[test]
fn oversized_unknown_length_stdout_is_bounded_and_child_is_reaped() {
    let parent = tempfile::tempdir().expect("test parent");
    let pid_file = parent.path().join("curl.pid");
    let pid_file_text = pid_file.to_string_lossy();
    assert!(!pid_file_text.contains('\''));
    let chunk = "x".repeat(8 * 1024);
    let script = format!(
        "#!/bin/sh\ncat >/dev/null\nprintf 'synthetic stderr marker' >&2\necho $$ > '{pid_file_text}'\nwhile :; do printf '%s' '{chunk}'; done\n"
    );
    let executable = write_executable(parent.path(), "curl-stub", &script);
    let scratch = Scratch::create_in(parent.path()).expect("private scratch");
    let scratch_path = scratch.dir.clone();
    let result = perform_in_scratch(
        "https://pipelines.example.test/session",
        &sample_request(),
        scratch,
        |config| {
            assert!(config.lines().any(|line| line == "output = \"-\""));
            let result = run_curl_with_executable(&executable, config);
            let entries = fs::read_dir(&scratch_path)
                .expect("scratch remains available during the exchange")
                .map(|entry| entry.expect("scratch entry").file_name())
                .collect::<Vec<_>>();
            assert_eq!(entries, [std::ffi::OsString::from("body")]);
            result
        },
    );

    assert!(matches!(result, Err(CurlFail::ResponseTooLarge)));
    assert!(!scratch_path.exists());
    let pid: u32 = fs::read_to_string(&pid_file)
        .expect("curl stub recorded its pid")
        .trim()
        .parse()
        .expect("curl stub pid is numeric");
    let still_running = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .expect("kill utility is available");
    assert!(!still_running.success(), "oversized curl child was reaped");
}

#[test]
fn curl_size_exit_is_not_an_empty_success_or_timeout() {
    assert!(matches!(
        classify_exit(Some(63), b"200".to_vec()),
        Err(CurlFail::ResponseTooLarge)
    ));
}

#[test]
fn curl_stdout_suffix_preserves_status_and_response_bytes() {
    let parent = tempfile::tempdir().expect("test parent");
    let executable = write_executable(
        parent.path(),
        "curl-stub",
        "#!/bin/sh\ncat >/dev/null\nprintf '%s' 'synthetic-body201'\n",
    );
    let (status, body) =
        run_curl_with_executable(&executable, "output = \"-\"").expect("bounded curl response");
    assert_eq!(status, 201);
    assert_eq!(body, b"synthetic-body");
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
        |_| Err(CurlFail::Timeout),
    );

    assert!(matches!(result, Err(CurlFail::Timeout)));
    assert!(!scratch_path.exists());
}

fn write_executable(parent: &Path, name: &str, contents: &str) -> PathBuf {
    let path = parent.join(name);
    fs::write(&path, contents).expect("write synthetic curl executable");
    let mut permissions = fs::metadata(&path)
        .expect("synthetic curl metadata")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&path, permissions).expect("make synthetic curl executable");
    path
}

//! Curl transport tests use only local fixtures.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use velnor_runner_github::{Method, SessionRequest};

use super::https::{CURL_ARGV, HttpsTransport, join_url, run_curl_in_home, trace_lines};
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
    assert_eq!(CURL_ARGV.first(), Some(&"-q"));
    assert!(!CURL_ARGV.iter().any(|arg| arg.contains("Authorization")));
}

#[test]
fn curl_ignores_retrying_curlrc_before_a_post() {
    let home = CurlHome::new();
    fs::write(
        home.path.join(".curlrc"),
        "retry = 1\nretry-all-errors\nretry-delay = 0\n",
    )
    .unwrap();
    let server = RetryFixture::start();
    let output = home.path.join("response");
    let config = format!(
        "request = \"POST\"\nurl = \"http://{}/create\"\noutput = \"{}\"\nwrite-out = \"%{{http_code}}\"\nmax-time = 5\n",
        server.address,
        output.display()
    );

    let result = run_curl_in_home(&config, &home.path);
    let requests = server.finish();

    assert!(matches!(result, Ok(503)));
    assert_eq!(requests, 1);
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

struct CurlHome {
    path: PathBuf,
}

impl CurlHome {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("velnor-curl-home-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self { path }
    }
}

impl Drop for CurlHome {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.path);
    }
}

struct RetryFixture {
    address: std::net::SocketAddr,
    requests: JoinHandle<usize>,
}

impl RetryFixture {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = thread::spawn(move || serve_retry_fixture(listener));
        Self { address, requests }
    }

    fn finish(self) -> usize {
        self.requests.join().unwrap()
    }
}

fn serve_retry_fixture(listener: TcpListener) -> usize {
    let (mut first, _) = listener.accept().unwrap();
    read_headers(&mut first).unwrap();
    respond(&mut first, 503).unwrap();
    listener.set_nonblocking(true).unwrap();

    let deadline = Instant::now() + Duration::from_millis(750);
    loop {
        match listener.accept() {
            Ok((mut retry, _)) => {
                read_headers(&mut retry).unwrap();
                respond(&mut retry, 409).unwrap();
                return 2;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return 1;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}

fn read_headers(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > 16 * 1024 {
            return Err(std::io::Error::other("request headers exceeded bound"));
        }
    }
    Ok(())
}

fn respond(stream: &mut TcpStream, status: u16) -> std::io::Result<()> {
    let reason = if status == 503 {
        "Service Unavailable"
    } else {
        "Conflict"
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

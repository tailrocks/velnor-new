use super::sniff_latest;
use super::transport::range::fetch_http_prefix_with_agent;
use super::transport::{
    MAX_REDIRECTS, decode_body, fetch_http_with_agent, fetch_text, resolve_redirect,
};
use crate::context::FETCH_CAP;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

struct MockResponse {
    status: u16,
    location: Option<String>,
    content_range: Option<String>,
    require_range: Option<String>,
    encoding: &'static str,
    chunked: bool,
    body: Vec<u8>,
    delay: Duration,
}

struct TestServer {
    url: String,
    thread: thread::JoinHandle<usize>,
}

struct TestDir(PathBuf);

impl TestDir {
    fn cleanup(mut self) -> std::io::Result<()> {
        fs::remove_dir_all(&self.0)?;
        self.0 = PathBuf::new();
        Ok(())
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty()
            && let Err(error) = fs::remove_dir_all(&self.0)
        {
            eprintln!("failed to clean test fixture {}: {error}", self.0.display());
        }
    }
}

fn mock_response(status: u16, body: Vec<u8>) -> MockResponse {
    MockResponse {
        status,
        location: None,
        content_range: None,
        require_range: None,
        encoding: "identity",
        chunked: false,
        body,
        delay: Duration::ZERO,
    }
}

fn redirect_response(body: Vec<u8>, encoding: &'static str, chunked: bool) -> MockResponse {
    MockResponse {
        status: 302,
        location: Some("/final".to_owned()),
        content_range: None,
        require_range: None,
        encoding,
        chunked,
        body,
        delay: Duration::ZERO,
    }
}

fn start_server(responses: Vec<MockResponse>) -> Result<TestServer, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let server_thread = thread::spawn(move || {
        let mut count = 0;
        for response in responses {
            let Some(mut stream) = accept_with_deadline(&listener) else {
                return count;
            };
            count += 1;
            if write_response(&mut stream, response).is_err() {
                return count;
            }
        }
        count
    });
    Ok(TestServer {
        url: format!("http://{address}/start"),
        thread: server_thread,
    })
}

fn accept_with_deadline(listener: &TcpListener) -> Option<TcpStream> {
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if let Ok((stream, _)) = listener.accept() {
            // Accepted sockets inherit the listener's nonblocking mode on
            // macOS; the request/response loops below assume blocking I/O.
            if stream.set_nonblocking(false).is_err() {
                continue;
            }
            return Some(stream);
        }
        thread::sleep(Duration::from_millis(5));
    }
    None
}

fn write_response(stream: &mut TcpStream, response: MockResponse) -> std::io::Result<()> {
    let mut request = [0_u8; 4096];
    let mut request_length = 0;
    loop {
        let amount = stream.read(&mut request[request_length..])?;
        if amount == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        request_length += amount;
        if request[..request_length]
            .windows(4)
            .any(|window| window == b"\r\n\r\n")
            || request_length == request.len()
        {
            break;
        }
    }
    let request_text = String::from_utf8_lossy(&request[..request_length]);
    let range_matches = response.require_range.as_ref().is_none_or(|expected| {
        request_text
            .lines()
            .any(|line| line.eq_ignore_ascii_case(expected))
    });
    let status = if range_matches { response.status } else { 416 };
    if !response.delay.is_zero() {
        thread::sleep(response.delay);
    }
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        302 => "Found",
        416 => "Range Not Satisfiable",
        _ => "Test Response",
    };
    write!(stream, "HTTP/1.1 {status} {reason}\r\n")?;
    if range_matches && let Some(location) = response.location {
        write!(stream, "Location: {location}\r\n")?;
    }
    if range_matches && !response.encoding.is_empty() {
        write!(stream, "Content-Encoding: {}\r\n", response.encoding)?;
    }
    if range_matches && let Some(content_range) = response.content_range {
        write!(stream, "Content-Range: {content_range}\r\n")?;
    }
    let body = if range_matches {
        response.body.as_slice()
    } else {
        b"missing expected Range header"
    };
    if range_matches && response.chunked {
        stream.write_all(b"Transfer-Encoding: chunked\r\n")?;
    } else {
        write!(stream, "Content-Length: {}\r\n", body.len())?;
    }
    stream.write_all(b"Connection: close\r\n\r\n")?;
    if range_matches && response.chunked {
        write!(stream, "{:X}\r\n", body.len())?;
        stream.write_all(body)?;
        stream.write_all(b"\r\n0\r\n\r\n")
    } else {
        stream.write_all(body)
    }
}

impl TestServer {
    fn finish(self) -> usize {
        self.thread.join().unwrap_or_default()
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(2)))
        .max_redirects(0)
        .build()
        .into()
}

fn redirect_fetch(
    body: Vec<u8>,
    encoding: &'static str,
    chunked: bool,
    follow: bool,
) -> (Result<String, String>, usize) {
    let first = redirect_response(body, encoding, chunked);
    let responses = if follow {
        vec![first, mock_response(200, b"final".to_vec())]
    } else {
        vec![first]
    };
    let server = start_server(responses);
    let Ok(server) = server else {
        return (Err("server setup failed".to_owned()), 0);
    };
    let result = fetch_http_with_agent(&agent(), &server.url, Duration::from_secs(2));
    (result, server.finish())
}

fn fetch_once(response: MockResponse) -> Result<String, String> {
    let server = start_server(vec![response]).map_err(|error| error.to_string())?;
    let result = fetch_text(&server.url);
    assert_eq!(server.finish(), 1);
    result
}

fn assert_body(response: MockResponse, expected: &str) {
    assert!(fetch_once(response).is_ok_and(|body| body == expected));
}

fn redirect_cap_case(byte: u8, encoding: &'static str, chunked: bool) {
    let body = |length| {
        let decoded = vec![byte; length];
        if encoding == "gzip" {
            gzip(&decoded)
        } else {
            decoded
        }
    };
    assert_eq!(
        redirect_fetch(body(FETCH_CAP), encoding, chunked, true),
        (Ok("final".to_owned()), 2)
    );
    let (result, requests) = redirect_fetch(body(FETCH_CAP + 1), encoding, chunked, false);
    assert!(result.is_err());
    assert_eq!(requests, 1);
}

fn gzip(input: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    let write_result = encoder.write_all(input);
    assert!(write_result.is_ok(), "gzip write: {write_result:?}");
    encoder.finish().unwrap_or_default()
}

fn temp_dir() -> std::io::Result<TestDir> {
    static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);
    let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "velnor-freshness-probe-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&path)?;
    Ok(TestDir(path))
}

fn file_url(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
}

fn file_fetch_watchdog(url: String) -> Result<Result<String, String>, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = fetch_text(&url);
        if let Err(error) = sender.send(result) {
            drop(error);
        }
    });
    receiver
        .recv_timeout(Duration::from_secs(1))
        .map_err(|error| format!("file source did not return promptly ({error})"))
}

mod http;

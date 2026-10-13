use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) fn chunked_server(total: usize, pause: Duration) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let base = format!(
        "http://{}",
        listener
            .local_addr()
            .expect("listener address should resolve")
    );
    let handle = thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _header = read_headers(&mut stream);
        let _header_write = stream.write_all(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
        );
        let chunk = vec![b'x'; 1024];
        let mut sent = 0;
        while sent < total {
            let len = (total - sent).min(chunk.len());
            if stream.write_all(format!("{len:x}\r\n").as_bytes()).is_err()
                || stream.write_all(&chunk[..len]).is_err()
                || stream.write_all(b"\r\n").is_err()
                || stream.flush().is_err()
            {
                return;
            }
            sent += len;
            if !pause.is_zero() {
                thread::sleep(pause);
            }
        }
        let _finished = stream.write_all(b"0\r\n\r\n");
    });
    (base, handle)
}

pub(crate) fn one_response_server(
    response: Vec<u8>,
    idle: Duration,
) -> (String, thread::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should be nonblocking");
    let base = format!(
        "http://{}",
        listener
            .local_addr()
            .expect("listener address should resolve")
    );
    let handle = thread::spawn(move || {
        let mut count = 0;
        let hard_stop = Instant::now() + Duration::from_secs(2);
        let mut idle_stop = None;
        while Instant::now() < hard_stop && idle_stop.is_none_or(|end| Instant::now() < end) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    count += 1;
                    let _request = read_headers(&mut stream);
                    if count == 1 {
                        let _response = stream.write_all(&response);
                    }
                    idle_stop = Some(Instant::now() + idle);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("listener accept failed: {error}"),
            }
        }
        count
    });
    (base, handle)
}

pub(crate) fn read_headers(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should be set");
    let mut bytes = Vec::new();
    let mut one = [0_u8; 1];
    while bytes.len() < 16 * 1024 {
        if stream.read_exact(&mut one).is_err() {
            break;
        }
        bytes.push(one[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    bytes
}

pub(crate) fn http_response(status: u16, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct RequestSummary {
    pub(crate) route: String,
    pub(crate) authorization_scheme: String,
    pub(crate) credential_matches: bool,
}

pub(crate) fn response_server(body: &[u8]) -> (String, thread::JoinHandle<RequestSummary>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("server should bind");
    let address = listener
        .local_addr()
        .expect("server address should resolve");
    let response_body = body.to_vec();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("request should arrive");
        let (head, _request_body) = read_request(&mut stream);
        let summary = request_summary(&head, "Bearer", "host-secret");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("response head should write");
        stream
            .write_all(&response_body)
            .expect("response body should write");
        summary
    });
    (format!("http://{address}"), server)
}

fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let _read_timeout = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 1024];
    let header_end = loop {
        let count = stream.read(&mut buffer).expect("request should read");
        assert_ne!(count, 0, "request must include a header");
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        assert!(
            bytes.len() < 64 * 1024,
            "request headers must remain bounded"
        );
    };
    let head = String::from_utf8(bytes[..header_end].to_vec()).expect("HTTP header is ASCII");
    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut request_body = bytes[header_end..].to_vec();
    while request_body.len() < content_length {
        let count = stream.read(&mut buffer).expect("request body should read");
        assert_ne!(count, 0, "declared request body must be complete");
        request_body.extend_from_slice(&buffer[..count]);
    }
    request_body.truncate(content_length);
    (head, request_body)
}

fn request_summary(head: &str, expected_scheme: &str, expected_credential: &str) -> RequestSummary {
    let route = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("")
        .to_owned();
    let authorization = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("authorization")
            .then_some(value.trim())
    });
    let (authorization_scheme, credential_matches) = authorization
        .and_then(|value| value.split_once(' '))
        .map(|(scheme, credential)| {
            (
                scheme.to_owned(),
                scheme == expected_scheme && credential == expected_credential,
            )
        })
        .unwrap_or_default();
    RequestSummary {
        route,
        authorization_scheme,
        credential_matches,
    }
}

pub(crate) fn process_exists(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .status()
        .is_ok_and(|status| status.success())
}

pub(crate) fn write_stub(path: &std::path::Path, script: &str) {
    fs::write(path, script).expect("stub should be written");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("stub should be executable");
}

pub(crate) struct TestDirectory {
    pub(crate) path: PathBuf,
}

impl TestDirectory {
    pub(crate) fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-async-curl-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test directory should be created");
        Self { path }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.path);
    }
}

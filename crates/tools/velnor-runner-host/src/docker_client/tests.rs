//! Bollard is constructed only from the configured absolute Unix socket.

use std::path::Path;
use std::time::Duration;
#[cfg(unix)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use super::{read_version_blocking_after, unix_socket_path};
use crate::docker_client::docker_deadline_after;
use crate::{HostError, connect_unix};

fn constructor_is_unix(tail: &str) -> bool {
    let Some(rest) = tail.strip_prefix("connect_with_unix") else {
        return false;
    };
    match rest.chars().next() {
        Some(ch) if ch.is_ascii_alphanumeric() || ch == '_' => false,
        Some(_) | None => true,
    }
}

fn assert_rejected(socket: &str) {
    assert!(matches!(connect_unix(socket), Err(HostError::Docker)));
}

#[test]
fn rejected_sockets_do_not_need_a_daemon() {
    assert_rejected("tcp://127.0.0.1:2375");
    assert_rejected("TCP://127.0.0.1:2375");
    assert_rejected("unix://var/run/docker.sock");
    assert_rejected("relative.sock");
    assert_rejected("");
}

#[test]
fn source_uses_only_the_unix_constructor() -> Result<(), std::io::Error> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err.to_string()))?;
    let source = std::fs::read_to_string(Path::new(&manifest).join("src/docker_client.rs"))?;
    let banned = format!("{}{}", "connect_with_", "local_defaults");
    assert!(!source.contains(&banned));
    let mut rest = source.as_str();
    let mut saw = false;
    while let Some(index) = rest.find("connect_with_") {
        let tail = &rest[index..];
        assert!(constructor_is_unix(tail), "unexpected bollard constructor");
        saw = true;
        rest = &tail["connect_with_".len()..];
    }
    assert!(saw);
    Ok(())
}

#[tokio::test]
async fn docker_deadline_preserves_results_and_fails_closed_on_timeout() {
    let response =
        docker_deadline_after(async { Ok::<_, u16>(200) }, Duration::from_millis(50)).await;
    assert_eq!(response, Ok(Ok(200)));

    let timeout = docker_deadline_after(
        tokio::time::sleep(Duration::from_millis(20)),
        Duration::from_millis(1),
    )
    .await;
    assert_eq!(timeout, Err(HostError::Docker));
}

#[test]
fn absolute_unix_path_only() {
    assert_eq!(
        unix_socket_path("unix:///var/run/docker.sock"),
        Ok("/var/run/docker.sock".to_owned())
    );
    assert_eq!(
        unix_socket_path("/var/run/docker.sock"),
        Ok("/var/run/docker.sock".to_owned())
    );
    assert_eq!(
        unix_socket_path("tcp://127.0.0.1:2375"),
        Err(HostError::Docker)
    );
    assert_eq!(
        unix_socket_path("TCP://127.0.0.1:2375"),
        Err(HostError::Docker)
    );
    assert_eq!(
        unix_socket_path("unix://var/run/docker.sock"),
        Err(HostError::Docker)
    );
    assert_eq!(unix_socket_path("relative.sock"), Err(HostError::Docker));
    assert_eq!(unix_socket_path("unix://"), Err(HostError::Docker));
    assert_eq!(unix_socket_path(""), Err(HostError::Docker));
    assert_eq!(
        unix_socket_path("ssh://example/tmp/docker.sock"),
        Err(HostError::Docker)
    );
}

#[cfg(unix)]
static NEXT_VERSION_SOCKET: AtomicUsize = AtomicUsize::new(0);

#[cfg(unix)]
fn version_socket_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-docker-version-{}-{}.sock",
        std::process::id(),
        NEXT_VERSION_SOCKET.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(unix)]
fn version_response() -> String {
    let body = r#"{"Version":"29.8.2","ApiVersion":"1.53","Os":"linux","Arch":"amd64"}"#;
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(unix)]
fn accept_and_read_request(mut stream: UnixStream) -> String {
    let _timeout = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut reader = BufReader::new(&mut stream);
    let mut request = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.is_empty() {
            break;
        }
        let finished = line == "\r\n" || line == "\n";
        request.push_str(&line);
        if finished {
            break;
        }
    }
    request
}

#[cfg(unix)]
fn mock_version_server(delay: Duration) -> (PathBuf, thread::JoinHandle<String>) {
    let path = version_socket_path();
    let listener = UnixListener::bind(&path).expect("bind mock Docker socket");
    let thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept Docker request");
        let request = accept_and_read_request(stream.try_clone().expect("clone socket"));
        thread::sleep(delay);
        let _response = stream.write_all(version_response().as_bytes());
        request
    });
    (path, thread)
}

#[cfg(unix)]
#[test]
fn version_probe_uses_only_get_and_returns_sanitized_server_fields() -> Result<(), String> {
    let (path, server) = mock_version_server(Duration::ZERO);
    let endpoint = format!("unix://{}", path.display());
    let version = read_version_blocking_after(&endpoint, Duration::from_secs(1))
        .map_err(|error| format!("Docker version request failed: {error}"))?;
    let request = server
        .join()
        .map_err(|_| "mock server panicked".to_owned())?;
    std::fs::remove_file(path).map_err(|error| error.to_string())?;

    let first_line = request.lines().next().unwrap_or_default();
    assert!(
        matches!(
            first_line,
            line if line == "GET /version HTTP/1.1"
                || (line.starts_with("GET /v1.") && line.ends_with("/version HTTP/1.1"))
        ),
        "unexpected request: {first_line}"
    );
    assert_eq!(version.server_version, "29.8.2");
    assert_eq!(version.api_version.as_deref(), Some("1.53"));
    assert_eq!(version.os.as_deref(), Some("linux"));
    assert_eq!(version.architecture.as_deref(), Some("amd64"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn version_probe_has_a_finite_deadline() -> Result<(), String> {
    let (path, server) = mock_version_server(Duration::from_millis(150));
    let endpoint = format!("unix://{}", path.display());
    let started = std::time::Instant::now();
    let result = read_version_blocking_after(&endpoint, Duration::from_millis(20));
    let elapsed = started.elapsed();
    let request = server
        .join()
        .map_err(|_| "mock server panicked".to_owned())?;
    std::fs::remove_file(path).map_err(|error| error.to_string())?;

    assert!(result.is_err());
    assert!(elapsed < Duration::from_millis(120));
    let first_line = request.lines().next().unwrap_or_default();
    assert!(
        first_line == "GET /version HTTP/1.1"
            || (first_line.starts_with("GET /v1.") && first_line.ends_with("/version HTTP/1.1")),
        "unexpected request: {first_line}"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn blocking_version_probe_rejects_an_existing_runtime() {
    assert!(read_version_blocking_after("unix:///unused", Duration::from_millis(20)).is_err());
}

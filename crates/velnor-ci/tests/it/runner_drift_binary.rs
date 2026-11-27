// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use velnor_model::RUNNER_PROTOCOL_VERSION;

fn drift(args: &[&str], api_url: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .arg("runner-drift")
        .args(args)
        .env_remove("GITHUB_API_URL");
    for proxy in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        command.env_remove(proxy);
    }
    command.env("NO_PROXY", "127.0.0.1");
    match api_url {
        Some(url) => command.env("GITHUB_TOKEN", "t").env("GITHUB_API_URL", url),
        None => command.env_remove("GITHUB_TOKEN"),
    };
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn the_reported_version_is_three_integers() {
    let parts: Vec<&str> = RUNNER_PROTOCOL_VERSION.split('.').collect();
    assert_eq!(parts.len(), 3, "{RUNNER_PROTOCOL_VERSION}");
    assert!(parts.iter().all(|part| part.parse::<u64>().is_ok()));
}

#[test]
fn a_newer_latest_tag_exits_1_with_the_behind_line() {
    let output = drift(&["--latest", "v9.9.9"], None);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout(&output),
        format!("runner protocol behind: {RUNNER_PROTOCOL_VERSION} < 9.9.9\n")
    );
}

#[test]
fn an_older_latest_tag_exits_0_with_the_current_line() {
    let output = drift(&["--latest", "v0.0.0"], None);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        format!("runner protocol current: {RUNNER_PROTOCOL_VERSION}\n")
    );
}

#[test]
fn an_unreadable_latest_tag_exits_2_with_nothing_on_stdout() {
    let output = drift(&["--latest", "v2.338"], None);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

#[test]
fn a_failed_lookup_exits_2_not_1() {
    let port = TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .port();
    let output = drift(&[], Some(&format!("http://127.0.0.1:{port}")));
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
}

#[test]
fn the_latest_release_is_read_from_the_api_and_a_newer_tag_exits_1() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    listener
        .set_nonblocking(true)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let url = format!(
        "http://{}",
        listener
            .local_addr()
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    let finished = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&finished);
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut request = Vec::new();
        while !stopped.load(Ordering::SeqCst) && Instant::now() < deadline {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            };
            stream
                .set_nonblocking(false)
                .and_then(|()| stream.set_read_timeout(Some(Duration::from_secs(1))))
                .unwrap_or_else(|e| unreachable!("{e}"));
            let mut chunk = [0_u8; 512];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => request.extend_from_slice(&chunk[..read]),
                }
            }
            let body = r#"{"tag_name":"v9.9.9"}"#;
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
            break;
        }
        String::from_utf8_lossy(&request).into_owned()
    });
    let output = drift(&[], Some(&url));
    finished.store(true, Ordering::SeqCst);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stdout(&output),
        format!("runner protocol behind: {RUNNER_PROTOCOL_VERSION} < 9.9.9\n")
    );
    let request = server
        .join()
        .unwrap_or_else(|_| unreachable!("stub panicked"));
    assert!(
        request.starts_with("GET /repos/actions/runner/releases/latest HTTP/1.1"),
        "{request}"
    );
}

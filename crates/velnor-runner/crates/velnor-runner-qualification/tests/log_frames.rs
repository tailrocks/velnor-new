// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![expect(
    clippy::unwrap_used,
    reason = "the external Docker fixture fails immediately on invalid test state"
)]

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::time::Duration;

use velnor_runner_qualification::read_logs;

fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![kind, 0, 0, 0];
    bytes.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn logs(body: Vec<u8>) -> Result<Vec<u8>, velnor_runner_qualification::QualificationError> {
    let directory = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("lf{}", rand_tag()));
    std::fs::create_dir(&directory).unwrap();
    let address = directory.join("d");
    let listener = UnixListener::bind(&address).unwrap();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0; 4096];
            let mut seen = Vec::new();
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buffer).unwrap_or(0);
                if n == 0 {
                    return;
                }
                seen.extend_from_slice(&buffer[..n]);
            }
            let request = String::from_utf8_lossy(&seen);
            let first = request.lines().next().unwrap_or_default();
            assert!(
                first.contains("/containers/abc/logs?") && first.contains("stderr=true"),
                "{first}"
            );
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.docker.raw-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            drop(stream.write_all(head.as_bytes()));
            drop(stream.write_all(&body));
        }
    });
    let docker = bollard::Docker::connect_with_socket(
        &address.to_string_lossy(),
        30,
        bollard::API_DEFAULT_VERSION,
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(read_logs(&docker, "abc", Duration::from_secs(10)));
    std::fs::remove_file(&address).unwrap();
    std::fs::remove_dir(&directory).unwrap();
    result
}

fn rand_tag() -> String {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}x{sequence}", std::process::id())
}

#[test]
fn complete_multi_frame_log_parses() {
    let mut body = frame(1, b"one\n");
    body.extend(frame(2, b""));
    body.extend(frame(1, b"two\n"));
    assert_eq!(logs(body).unwrap(), b"one\ntwo\n");
}

#[test]
fn trailing_header_without_payload_is_rejected() {
    let mut body = frame(1, b"Docker version 1, build abc\n");
    body.extend([2, 0, 0, 0, 0, 0, 0, 5]);
    assert!(logs(body).is_err());
}

#[test]
fn stdout_header_longer_than_remaining_payload_is_rejected() {
    let mut body = frame(1, b"ok\n");
    body.extend([1, 0, 0, 0, 0, 0, 0, 9]);
    body.extend(b"abc");
    assert!(logs(body.clone()).is_err());
    body.truncate(body.len() - 3);
    assert!(logs(body).is_err());
}

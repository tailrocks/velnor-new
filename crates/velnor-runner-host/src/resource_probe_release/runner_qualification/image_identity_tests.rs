// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::time::Instant;

use super::daemon;
use crate::docker_client::connect_unix;
use crate::error::HostError;

const PROBE_BOUND: Duration = Duration::from_secs(5);
const SLACK: Duration = Duration::from_secs(1);
const OUTER_GUARD: Duration = Duration::from_secs(60);
const ANSWER: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";

fn socket_path(stalled: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-probe-stall-{}-{}.sock",
        std::process::id(),
        stalled.trim_start_matches('/')
    ))
}

async fn request_path(stream: &mut UnixStream) -> Option<String> {
    let mut head = Vec::new();
    let mut chunk = [0u8; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        head.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8_lossy(&head)
        .split_whitespace()
        .nth(1)
        .map(str::to_owned)
}

async fn serve(listener: UnixListener, stalled: &'static str, seen: Arc<Mutex<Vec<String>>>) {
    while let Ok((mut stream, _)) = listener.accept().await {
        let seen = Arc::clone(&seen);
        tokio::spawn(async move {
            let Some(path) = request_path(&mut stream).await else {
                return;
            };
            seen.lock().expect("seen request paths").push(path.clone());
            if path.ends_with(stalled) {
                std::future::pending::<()>().await;
            }
            let _ = stream.write_all(ANSWER).await;
        });
    }
}

async fn probe_with_stalled_operation(
    stalled: &'static str,
) -> (
    Result<Result<super::DaemonEvidence, HostError>, tokio::time::error::Elapsed>,
    Duration,
    Vec<String>,
) {
    let path = socket_path(stalled);
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind stub daemon socket");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let server = tokio::spawn(serve(listener, stalled, Arc::clone(&seen)));
    let socket = path.to_string_lossy().into_owned();
    let docker = connect_unix(&socket).expect("docker client for stub socket");

    let started = Instant::now();
    let outcome = tokio::time::timeout(OUTER_GUARD, daemon(&docker)).await;
    let elapsed = started.elapsed();

    server.abort();
    let _ = std::fs::remove_file(&path);
    let seen = seen.lock().expect("seen request paths").clone();
    (outcome, elapsed, seen)
}

async fn assert_given_up_within_bound(stalled: &'static str) {
    let (outcome, elapsed, seen) = probe_with_stalled_operation(stalled).await;
    assert!(
        seen.iter().any(|path| path.ends_with(stalled)),
        "stub daemon never received {stalled}, saw {seen:?}"
    );
    assert!(
        matches!(outcome, Ok(Err(HostError::DockerTimeout))),
        "probe stalled on {stalled} must give up with DockerTimeout, got {outcome:?}"
    );
    assert!(
        elapsed <= PROBE_BOUND + SLACK,
        "probe took {elapsed:?}, bound is {PROBE_BOUND:?}"
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_daemon_that_never_answers_version_is_given_up_within_the_probe_bound() {
    assert_given_up_within_bound("/version").await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_daemon_that_never_answers_info_is_given_up_within_the_probe_bound() {
    assert_given_up_within_bound("/info").await;
}

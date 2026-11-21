// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use velnor_runner_image_manifest::{ImageCandidateArchive, ImageCandidateOci};

use super::{daemon, inspect};
use crate::docker_client::connect_unix;
use crate::error::HostError;
use crate::launch::ProjectedCandidate;

const PROBE_BOUND: Duration = Duration::from_secs(5);
const SLACK: Duration = Duration::from_secs(1);
const OUTER_GUARD: Duration = Duration::from_secs(60);
const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const ANSWER: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";

struct Stub {
    path: PathBuf,
    docker: Docker,
    server: JoinHandle<()>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    fn start(stalled: &'static str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "velnor-probe-stall-{}-{}.sock",
            std::process::id(),
            stalled.trim_start_matches('/')
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind stub daemon socket");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let server = tokio::spawn(serve(listener, stalled, Arc::clone(&seen)));
        let docker = connect_unix(&path.to_string_lossy()).expect("docker client for stub socket");
        Self {
            path,
            docker,
            server,
            seen,
        }
    }

    fn finish(self) -> Vec<String> {
        self.server.abort();
        let _ = std::fs::remove_file(&self.path);
        self.seen.lock().expect("seen request paths").clone()
    }
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

fn assert_gave_up_within_bound<T: std::fmt::Debug>(
    outcome: &Result<Result<T, HostError>, tokio::time::error::Elapsed>,
    elapsed: Duration,
    seen: &[String],
    stalled: &str,
) {
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

fn candidate() -> ProjectedCandidate {
    ProjectedCandidate {
        archive: ImageCandidateArchive {
            sha256: "0".repeat(64),
            size_bytes: 1,
        },
        oci: ImageCandidateOci {
            archive_index_digest: "0".repeat(64),
            archive_index_size_bytes: 1,
            platform_manifest_digest: DIGEST.to_owned(),
            platform_manifest_size_bytes: 1,
            image_configuration_digest: DIGEST.to_owned(),
            image_configuration_size_bytes: 1,
            layer_digests: Vec::new(),
        },
        configuration: serde_json::Value::Object(serde_json::Map::new()),
        diff_ids: Vec::new(),
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_daemon_that_never_answers_version_is_given_up_within_the_probe_bound() {
    let stub = Stub::start("/version");
    let started = Instant::now();
    let outcome = tokio::time::timeout(OUTER_GUARD, daemon(&stub.docker)).await;
    let elapsed = started.elapsed();
    let seen = stub.finish();
    assert_gave_up_within_bound(&outcome, elapsed, &seen, "/version");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_daemon_that_never_answers_info_is_given_up_within_the_probe_bound() {
    let stub = Stub::start("/info");
    let started = Instant::now();
    let outcome = tokio::time::timeout(OUTER_GUARD, daemon(&stub.docker)).await;
    let elapsed = started.elapsed();
    let seen = stub.finish();
    assert_gave_up_within_bound(&outcome, elapsed, &seen, "/info");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_daemon_that_never_answers_image_inspect_is_given_up_within_the_probe_bound() {
    let stub = Stub::start("/json");
    let expected = candidate();
    let started = Instant::now();
    let outcome = tokio::time::timeout(OUTER_GUARD, inspect(&stub.docker, &expected)).await;
    let elapsed = started.elapsed();
    let seen = stub.finish();
    assert_gave_up_within_bound(&outcome, elapsed, &seen, "/json");
}

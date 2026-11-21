// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::time::Duration;

use tokio::net::UnixListener;
use tokio::time::Instant;

use super::daemon;
use crate::docker_client::connect_unix;
use crate::error::HostError;

const PROBE_BOUND: Duration = Duration::from_secs(5);
const SLACK: Duration = Duration::from_secs(1);
const OUTER_GUARD: Duration = Duration::from_secs(60);

fn socket_path() -> PathBuf {
    std::env::temp_dir().join(format!("velnor-probe-stall-{}.sock", std::process::id()))
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_daemon_that_never_answers_is_given_up_within_the_probe_bound() {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind stub daemon socket");
    let held = tokio::spawn(async move {
        let mut streams = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            streams.push(stream);
        }
    });
    let socket = path.to_string_lossy().into_owned();
    let docker = connect_unix(&socket).expect("docker client for stub socket");

    let started = Instant::now();
    let outcome = tokio::time::timeout(OUTER_GUARD, daemon(&docker)).await;
    let elapsed = started.elapsed();

    held.abort();
    let _ = std::fs::remove_file(&path);
    assert!(
        matches!(outcome, Ok(Err(HostError::DockerTimeout))),
        "probe with no bound must give up with DockerTimeout, got {outcome:?}"
    );
    assert!(
        elapsed <= PROBE_BOUND + SLACK,
        "probe took {elapsed:?}, bound is {PROBE_BOUND:?}"
    );
}

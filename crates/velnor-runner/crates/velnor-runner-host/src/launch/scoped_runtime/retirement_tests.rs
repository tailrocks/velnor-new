// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use super::retire_pending_pair;
use crate::journal::Journal;
use crate::launch::VerifiedWorkerImages;
use crate::launch::bootstrap::unbound_pair_fixtures::{
    FAILED, fixture::FixtureDocker, seed,
};
use crate::launch::scoped_runtime::scoped_prepare_context;

const IMAGE_ID: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test(flavor = "multi_thread")]
async fn the_turn_deadline_reaches_the_retirement_of_a_pending_pair() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    let images = VerifiedWorkerImages::for_test(&engine, &"1".repeat(40), IMAGE_ID, IMAGE_ID)?;
    fixture.hang_after(format!(
        "DELETE /containers/{}",
        FAILED.container_id("runner")
    ));

    let started = Instant::now();
    let context = scoped_prepare_context(
        &docker,
        &scope,
        &journal,
        &(),
        &engine,
        &images,
        started + Duration::from_millis(500),
    );
    tokio::time::timeout(
        Duration::from_secs(60),
        retire_pending_pair(context, FAILED.reservation),
    )
    .await?;

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use crate::journal::Journal;

use super::super::{abandon_and_retire_unbound_pair_before, retire_uncertain_unbound_pairs};
use super::fixture::FixtureDocker;
use super::seed::{self, RESERVATION};
use super::{FAILED, TestResult};

#[tokio::test(flavor = "multi_thread")]
async fn retirement_against_a_stalled_docker_returns_at_the_turn_deadline() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    fixture.hang_after(format!(
        "DELETE /containers/{}",
        FAILED.container_id("runner")
    ));

    let started = Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_secs(60),
        abandon_and_retire_unbound_pair_before(
            started + Duration::from_millis(500),
            &docker,
            &scope,
            &engine,
            FAILED.reservation,
        ),
    )
    .await?;

    assert!(outcome.is_err());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        "uncertain/00/uncertain/0/1/none/"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_of_uncertain_pairs_stops_at_the_deadline_when_docker_stalls() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let fixture = FixtureDocker::start(&socket, &[FAILED])?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, &[FAILED]).await?;
    scope.abandon_unbound_pair(FAILED.reservation).await?;
    fixture.hang_after(format!(
        "DELETE /containers/{}",
        FAILED.container_id("runner")
    ));

    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(60),
        retire_uncertain_unbound_pairs(
            started + Duration::from_millis(500),
            &docker,
            &scope,
            &engine,
        ),
    )
    .await??;

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        seed::text(&journal, RESERVATION, FAILED.reservation).await?,
        "uncertain/00/uncertain/0/1/none/"
    );
    Ok(())
}

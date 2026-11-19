// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use tokio::time::Instant;

use super::settle_removed;
use crate::config::RepositoryConnection;
use crate::host_controller::{fixture_config, removed_fixture};
use crate::journal::Journal;
use crate::scale_set::{EnsureError, RunnerProver};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type Script = [((i64, i64), Result<bool, EnsureError>)];

/// What GitHub said about each `(scale_set_id, runner_id)`; anything unlisted is "not our set".
struct Scripted(BTreeMap<(i64, i64), Result<bool, EnsureError>>);

impl RunnerProver for Scripted {
    fn prove_absent(&mut self, scale_set_id: i64, runner_id: i64) -> Result<bool, EnsureError> {
        self.0
            .get(&(scale_set_id, runner_id))
            .copied()
            .unwrap_or(Ok(false))
    }
}

struct World {
    _dir: tempfile::TempDir,
    state: std::path::PathBuf,
    journal: Journal,
    binding: RepositoryConnection,
}

async fn world(runners: &[(i64, &str)]) -> TestResult<World> {
    let dir = tempfile::tempdir()?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    let connection = journal.connection().await?;
    for (set, runner) in runners {
        connection
            .execute(
                "INSERT INTO intents (kind, subject, state, scale_set_id, github_runner_id) VALUES ('launch', 'legacy', 'done', ?1, ?2)",
                (*set, (*runner).to_owned()),
            )
            .await?;
    }
    let binding = fixture_config("conn-test-1")?.connections()[0].clone();
    let state = dir.path().join("state");
    std::fs::create_dir(&state)?;
    Ok(World {
        _dir: dir,
        state,
        journal,
        binding,
    })
}

async fn settle(world: &World, script: &Script) -> Result<usize, crate::error::HostError> {
    let prover = Scripted(script.iter().copied().collect());
    settle_removed(
        &world.journal,
        &world.state,
        &world.binding,
        Instant::now() + Duration::from_secs(30),
        move || Ok(prover),
    )
    .await
}

async fn proven(world: &World) -> TestResult<Vec<bool>> {
    Ok(world
        .journal
        .rows()
        .await?
        .iter()
        .map(|row| row.cleanup_proven)
        .collect())
}

fn removed(world: &World) {
    removed_fixture(Path::new(&world.state), &world.binding);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_confirmed_deregistration_settles_the_legacy_row() -> TestResult {
    let world = world(&[(10, "77")]).await?;
    removed(&world);
    assert_eq!(settle(&world, &[((10, 77), Ok(true))]).await?, 1);
    assert_eq!(proven(&world).await?, [true]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_deletion_settles_nothing() -> TestResult {
    for failure in [
        EnsureError::Uncertain,
        EnsureError::Forbidden,
        EnsureError::Unauthorized,
    ] {
        let world = world(&[(10, "77")]).await?;
        removed(&world);
        assert_eq!(settle(&world, &[((10, 77), Err(failure))]).await?, 0);
        assert_eq!(proven(&world).await?, [false], "{failure:?}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_runners_that_were_confirmed_are_settled() -> TestResult {
    let world = world(&[(10, "77"), (10, "78"), (10, "79")]).await?;
    removed(&world);
    let script = [
        ((10, 77), Ok(true)),
        ((10, 78), Err(EnsureError::Uncertain)),
        ((10, 79), Ok(false)),
    ];
    assert_eq!(settle(&world, &script).await?, 1);
    assert_eq!(proven(&world).await?, [true, false, false]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_later_run_keeps_what_an_earlier_run_settled() -> TestResult {
    let world = world(&[(10, "77"), (10, "78")]).await?;
    removed(&world);
    assert_eq!(settle(&world, &[((10, 77), Ok(true))]).await?, 1);
    assert_eq!(settle(&world, &[((10, 78), Ok(true))]).await?, 1);
    assert_eq!(settle(&world, &[((10, 78), Ok(true))]).await?, 0);
    assert_eq!(proven(&world).await?, [true, true]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removal_without_a_current_drain_proof_settles_nothing() -> TestResult {
    let world = world(&[(10, "77")]).await?;
    assert_eq!(settle(&world, &[((10, 77), Ok(true))]).await?, 0);
    assert_eq!(proven(&world).await?, [false]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_confirmation_for_another_scale_set_or_runner_settles_nothing() -> TestResult {
    let world = world(&[(10, "77")]).await?;
    removed(&world);
    let script = [((11, 77), Ok(true)), ((10, 770), Ok(true))];
    assert_eq!(settle(&world, &script).await?, 0);
    assert_eq!(proven(&world).await?, [false]);
    Ok(())
}

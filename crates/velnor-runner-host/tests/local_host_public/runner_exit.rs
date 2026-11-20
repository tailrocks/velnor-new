// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use velnor_runner_host::HostError;
use velnor_runner_host::local::start_pair;

use super::proxy::{Faults, Proxy, Wait};
use super::scenario::{Scenario, finish_job};
use super::support::TestResult;

fn every_wait_is_killed(_call: usize) -> Wait {
    Wait::Killed
}

fn every_wait_fails(_call: usize) -> Wait {
    Wait::Fail
}

fn every_wait_misses(_call: usize) -> Wait {
    Wait::Missing
}

async fn host_ends_with(waits: fn(usize) -> Wait) -> TestResult<Option<Result<(), HostError>>> {
    let Some(mut scenario) = Scenario::new_with_runtime().await? else {
        return Ok(None);
    };
    let proxy = Proxy::start(&scenario.fixture.endpoint, Faults::new(waits, 0))?;
    let plan = scenario.plan(1, 0)?;
    scenario.start_on(proxy.docker.clone(), plan);
    let ended = scenario.ended().await;
    scenario.finish().await?;
    Ok(Some(ended?))
}

#[tokio::test]
async fn local_host_fails_when_the_runner_container_exits_non_zero() -> TestResult<()> {
    if let Some(ended) = host_ends_with(every_wait_is_killed).await? {
        assert_eq!(ended, Err(HostError::RunnerExited));
    }
    Ok(())
}

#[tokio::test]
async fn local_host_fails_when_the_runner_container_is_removed_underneath_it() -> TestResult<()> {
    if let Some(ended) = host_ends_with(every_wait_misses).await? {
        assert_eq!(ended, Err(HostError::Docker));
    }
    Ok(())
}

#[tokio::test]
async fn local_host_fails_when_the_wait_itself_fails() -> TestResult<()> {
    if let Some(ended) = host_ends_with(every_wait_fails).await? {
        assert_eq!(ended, Err(HostError::Docker));
    }
    Ok(())
}

#[tokio::test]
async fn local_a_finished_guest_job_ends_the_runner_container_with_exit_zero() -> TestResult<()> {
    let Some(scenario) = Scenario::new().await? else {
        return Ok(());
    };
    let docker = scenario.fixture.docker.clone();
    let pair = Box::pin(start_pair(&docker, &scenario.fixture.request())).await?;
    let finished = tokio::time::timeout(
        Duration::from_secs(30),
        finish_job(&docker, pair.runner_id()),
    )
    .await;
    let state = docker
        .inspect_container(pair.runner_id(), None)
        .await?
        .state
        .unwrap_or_default();
    scenario.finish().await?;
    assert!(
        matches!(finished, Ok(Ok(()))),
        "the runner did not exit 0: {finished:?}"
    );
    assert_eq!(state.running, Some(false));
    assert_eq!(state.exit_code, Some(0));
    Ok(())
}

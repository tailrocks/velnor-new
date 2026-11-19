// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::HostError;

use super::proxy::{Faults, Proxy, Wait};
use super::scenario::Scenario;
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
    let Some(mut scenario) = Scenario::new().await? else {
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

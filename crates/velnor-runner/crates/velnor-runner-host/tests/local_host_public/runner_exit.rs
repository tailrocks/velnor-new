// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::query_parameters::RemoveContainerOptionsBuilder;
use velnor_runner_host::HostError;

use super::proxy::{Faults, Proxy, Wait};
use super::scenario::{Scenario, wait_for_runner};
use super::support::{Program, TestResult};

fn every_wait_misses(_call: usize) -> Wait {
    Wait::Missing
}

#[tokio::test]
async fn local_host_fails_when_the_runner_container_exits_non_zero() -> TestResult<()> {
    let Some(mut scenario) = Scenario::running(Program::Exit).await? else {
        return Ok(());
    };
    let plan = scenario.plan(1, 0)?;
    scenario.start(plan);
    let ended = scenario.stop().await;
    scenario.finish().await?;
    assert_eq!(ended?, Err(HostError::RunnerExited));
    Ok(())
}

#[tokio::test]
async fn local_host_fails_when_the_runner_container_is_removed_underneath_it() -> TestResult<()> {
    let Some(mut scenario) = Scenario::new().await? else {
        return Ok(());
    };
    let plan = scenario.plan(1, 0)?;
    scenario.start(plan);
    let runner = wait_for_runner(
        &scenario.fixture.docker,
        &scenario.fixture.environment,
        "trusted",
        &[],
    )
    .await?;
    scenario
        .fixture
        .docker
        .remove_container(
            &runner.runner,
            Some(RemoveContainerOptionsBuilder::new().force(true).build()),
        )
        .await?;
    let ended = scenario.stop().await;
    scenario.finish().await?;
    assert!(
        matches!(ended?, Err(HostError::RunnerExited | HostError::Docker)),
        "removing the runner must not end the host with Ok"
    );
    Ok(())
}

#[tokio::test]
async fn local_host_fails_when_the_wait_answers_that_the_container_is_gone() -> TestResult<()> {
    let Some(mut scenario) = Scenario::new().await? else {
        return Ok(());
    };
    let proxy = Proxy::start(&scenario.fixture.endpoint, Faults::new(every_wait_misses, 0))?;
    let plan = scenario.plan(1, 0)?;
    scenario.start_on(proxy.docker.clone(), plan);
    let ended = scenario.stop().await;
    scenario.finish().await?;
    assert_eq!(ended?, Err(HostError::Docker));
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::HostError;
use velnor_runner_host::local::supervise;

use std::time::{Duration, Instant};

use super::proxy::{Faults, Proxy, Wait};
use super::scenario::Scenario;
use super::support::TestResult;

fn mints(events: &[String]) -> usize {
    events
        .iter()
        .filter(|event| event.starts_with("mint:"))
        .count()
        / 2
}

async fn supervise_until(
    scenario: &Scenario,
    faults: std::sync::Arc<Faults>,
    minted: usize,
) -> TestResult<Result<(), HostError>> {
    let proxy = Proxy::start(faults)?;
    let plan = scenario.plan(1, 0)?;
    let state = scenario.state.path().to_path_buf();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        supervise(proxy.docker.clone(), plan, &state, async move {
            let _stopped = stopped.await;
        })
        .await
    });
    let started = Instant::now();
    while !task.is_finished()
        && mints(&scenario.fake().events()) < minted
        && started.elapsed() < Duration::from_secs(120)
    {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let _sent = stop.send(());
    Ok(tokio::time::timeout(Duration::from_secs(120), task).await??)
}

#[tokio::test]
async fn local_pair_docker_failures_are_retried_three_times_before_the_host_gives_up()
-> TestResult<()> {
    let scenario = Scenario::new().await?;
    let mut plan = scenario.plan(1, 0)?;
    plan.runner_image = format!("sha256:{}", "0".repeat(64));
    let docker = scenario.fixture.docker.clone();
    let state = scenario.state.path().to_path_buf();
    let ended = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        supervise(docker, plan, &state, std::future::pending()),
    )
    .await;
    let events = scenario.fake().events();
    scenario.finish().await?;
    let mints = events
        .iter()
        .filter(|event| event.starts_with("mint:"))
        .count();
    assert_eq!(ended?, Err(HostError::Docker));
    assert_eq!(mints, 2 * 3, "{events:?}");
    for agent in ["agent-1", "agent-2", "agent-3"] {
        let deleted = format!("delete:trusted:{agent}");
        assert!(events.contains(&deleted), "{events:?}");
    }
    Ok(())
}

fn first_wait_fails(call: usize) -> Wait {
    if call == 0 { Wait::Fail } else { Wait::Forward }
}

fn every_wait_fails(_call: usize) -> Wait {
    Wait::Fail
}

fn failures_alternate_with_successes(call: usize) -> Wait {
    match (call < 6, call % 2) {
        (true, 0) => Wait::Fail,
        (true, _) => Wait::Finish,
        (false, _) => Wait::Forward,
    }
}

#[tokio::test]
async fn local_slot_removes_a_failed_pair_before_it_mints_the_replacement() -> TestResult<()> {
    let scenario = Scenario::new().await?;
    let faults = Faults::new(first_wait_fails, 2);
    let ended = supervise_until(&scenario, faults, 2).await;
    let events = scenario.fake().events();
    scenario.finish().await?;
    assert_eq!(ended?, Ok(()), "{events:?}");
    assert_eq!(mints(&events), 2, "{events:?}");
    assert!(
        events
            .iter()
            .filter(|event| event.contains("live_trusted="))
            .all(|event| event.ends_with("live_trusted=0")),
        "{events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn local_slot_counts_a_failed_removal_toward_the_retry_bound_and_mints_nothing_more()
-> TestResult<()> {
    let scenario = Scenario::new().await?;
    let faults = Faults::new(every_wait_fails, 3);
    let ended = supervise_until(&scenario, faults, 100).await;
    let events = scenario.fake().events();
    scenario.finish().await?;
    assert_eq!(ended?, Err(HostError::Docker), "{events:?}");
    assert_eq!(mints(&events), 1, "{events:?}");
    Ok(())
}

#[tokio::test]
async fn local_slot_counts_only_consecutive_failures_toward_the_retry_bound() -> TestResult<()> {
    let scenario = Scenario::new().await?;
    let faults = Faults::new(failures_alternate_with_successes, 0);
    let ended = supervise_until(&scenario, faults, 7).await;
    let events = scenario.fake().events();
    scenario.finish().await?;
    assert_eq!(ended?, Ok(()), "{events:?}");
    assert!(mints(&events) >= 7, "{events:?}");
    Ok(())
}

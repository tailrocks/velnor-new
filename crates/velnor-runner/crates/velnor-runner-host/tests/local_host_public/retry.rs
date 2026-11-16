// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::HostError;
use velnor_runner_host::local::supervise;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::proxy::{Faults, Proxy, Wait};
use super::runner;
use super::scenario::Scenario;
use super::support::{TestResult, labelled};

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

#[tokio::test]
async fn local_transient_mint_failure_is_retried_and_leaves_the_other_slot_running()
-> TestResult<()> {
    let mut scenario = Scenario::new().await?;
    let plan = scenario.plan(2, 0)?;
    scenario.fake().mint_failures.store(1, Ordering::SeqCst);
    scenario.start(plan);
    let run = async {
        let first = runner(&scenario, "trusted", &[]).await?;
        let second = runner(&scenario, "trusted", std::slice::from_ref(&first.agent)).await?;
        TestResult::Ok((first.agent, second.agent))
    }
    .await;
    let stopped = scenario.stop().await?;
    let events = scenario.fake().events();
    scenario.finish().await?;
    let (first, second) = run?;
    assert_ne!(first, second);
    assert_eq!(stopped, Ok(()), "{events:?}");
    assert!(events.contains(&"mint-failed".to_owned()), "{events:?}");
    Ok(())
}

#[tokio::test]
async fn local_shutdown_does_not_wait_for_a_stalled_mint() -> TestResult<()> {
    let mut scenario = Scenario::new().await?;
    let plan = scenario.plan(1, 0)?;
    scenario.fake().stall_mint.store(true, Ordering::SeqCst);
    scenario.start(plan);
    let started = Instant::now();
    while !scenario
        .fake()
        .events()
        .contains(&"mint-stalled".to_owned())
        && started.elapsed() < Duration::from_secs(60)
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let requested = Instant::now();
    let stopped = tokio::time::timeout(Duration::from_secs(30), scenario.stop()).await;
    let seconds = requested.elapsed().as_secs();
    let events = scenario.fake().events();
    scenario.finish().await?;
    assert!(events.contains(&"mint-stalled".to_owned()), "{events:?}");
    assert_eq!(stopped??, Ok(()), "{seconds}s");
    Ok(())
}

#[tokio::test]
async fn local_agent_minted_before_any_container_exists_is_deleted_by_the_next_sweep()
-> TestResult<()> {
    let scenario = Scenario::new().await?;
    let faults = Faults::new(|_| Wait::Forward, 0);
    faults.hang_creations.store(true, Ordering::SeqCst);
    let proxy = Proxy::start(std::sync::Arc::clone(&faults))?;
    let state = scenario.state.path().to_path_buf();
    let killed = {
        let docker = proxy.docker.clone();
        let plan = scenario.plan(1, 0)?;
        let state = state.clone();
        tokio::spawn(async move { supervise(docker, plan, &state, std::future::pending()).await })
    };
    let started = Instant::now();
    while !scenario
        .fake()
        .events()
        .iter()
        .any(|event| event.starts_with("mint:agent-1"))
        && started.elapsed() < Duration::from_secs(60)
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    killed.abort();
    let _joined = killed.await;
    let before = scenario.fake().events();
    let unlabelled = labelled(&scenario.fixture.docker, &scenario.fixture.environment).await?;
    let docker = scenario.fixture.docker.clone();
    let plan = scenario.plan(1, 0)?;
    let ended = tokio::time::timeout(
        Duration::from_secs(120),
        supervise(docker, plan, &state, async {}),
    )
    .await;
    let events = scenario.fake().events();
    scenario.finish().await?;
    assert_eq!(unlabelled, (0, 0), "{before:?}");
    assert_eq!(ended?, Ok(()), "{events:?}");
    assert!(
        events.contains(&"delete:trusted:agent-1".to_owned()),
        "{events:?}"
    );
    Ok(())
}

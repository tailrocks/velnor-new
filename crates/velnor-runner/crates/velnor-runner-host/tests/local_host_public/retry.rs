// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::HostError;
use velnor_runner_host::local::supervise;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::proxy::{Faults, Proxy, Wait};
use super::runner;
use super::scenario::{Scenario, finish_job};
use super::support::{TestResult, labelled};

fn minted_names(events: &[String]) -> Vec<String> {
    let mut names: Vec<String> = events
        .iter()
        .filter_map(|event| event.strip_prefix("mint:"))
        .filter_map(|rest| rest.split(':').next().map(str::to_owned))
        .collect();
    names.dedup();
    names
}

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
    assert_eq!(minted_names(&events).len(), 3, "{events:?}");
    for agent in minted_names(&events) {
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
    let cleared = !scenario.state.path().join("minted-agents.log").exists();
    scenario.finish().await?;
    assert!(events.contains(&"mint-stalled".to_owned()), "{events:?}");
    assert_eq!(stopped??, Ok(()), "{seconds}s");
    let registered: Vec<&str> = events
        .iter()
        .filter_map(|event| event.strip_prefix("registered:"))
        .collect();
    assert_eq!(registered.len(), 1, "{events:?}");
    assert!(
        events.contains(&format!("delete:trusted:{}", registered[0])),
        "{events:?}"
    );
    assert!(cleared, "{events:?}");
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
        .any(|event| event.starts_with("mint:"))
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
    let names = minted_names(&events);
    assert!(!names.is_empty(), "{events:?}");
    for name in names {
        assert!(
            events.contains(&format!("delete:trusted:{name}")),
            "{events:?}"
        );
    }
    Ok(())
}

async fn wait_for_event(scenario: &Scenario, event: &str) {
    let started = Instant::now();
    while !scenario.fake().events().iter().any(|seen| seen == event)
        && started.elapsed() < Duration::from_secs(60)
    {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
async fn local_slot_retries_the_agent_retirement_after_its_pair_was_removed() -> TestResult<()> {
    let mut scenario = Scenario::new().await?;
    scenario.fake().delete_failures.store(1, Ordering::SeqCst);
    let plan = scenario.plan(1, 0)?;
    scenario.start(plan);
    let run = async {
        let first = super::runner(&scenario, "trusted", &[]).await?;
        finish_job(&scenario.fixture.docker, &first.runner).await?;
        wait_for_event(&scenario, &format!("delete:trusted:{}", first.agent)).await;
        TestResult::Ok(first.agent)
    }
    .await;
    let stopped = scenario.stop().await?;
    let events = scenario.fake().events();
    scenario.finish().await?;
    let agent = run?;
    assert!(
        events.contains(&format!("delete_failed:trusted:{agent}")),
        "{events:?}"
    );
    assert!(
        events.contains(&format!("delete:trusted:{agent}")),
        "{events:?}"
    );
    assert_eq!(stopped, Ok(()), "{events:?}");
    Ok(())
}

#[tokio::test]
async fn local_slot_keeps_the_start_failure_when_retiring_the_agent_also_fails() -> TestResult<()> {
    let scenario = Scenario::new().await?;
    scenario
        .fake()
        .delete_failures
        .store(usize::MAX, Ordering::SeqCst);
    let mut plan = scenario.plan(1, 0)?;
    plan.runner_image = format!("sha256:{}", "0".repeat(64));
    let docker = scenario.fixture.docker.clone();
    let state = scenario.state.path().to_path_buf();
    let ended = tokio::time::timeout(
        Duration::from_secs(300),
        supervise(docker, plan, &state, std::future::pending()),
    )
    .await;
    let events = scenario.fake().events();
    scenario.fake().delete_failures.store(0, Ordering::SeqCst);
    scenario.finish().await?;
    assert_eq!(ended?, Err(HostError::Docker), "{events:?}");
    let names = minted_names(&events);
    assert_eq!(names.len(), 1, "{events:?}");
    let failed = format!("delete_failed:trusted:{}", names[0]);
    assert!(
        events.iter().filter(|event| **event == failed).count() >= 3,
        "{events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn local_slot_retires_the_agent_of_a_pair_that_failed_to_start_before_minting_again()
-> TestResult<()> {
    let scenario = Scenario::new().await?;
    let mut plan = scenario.plan(1, 0)?;
    plan.runner_image = format!("sha256:{}", "0".repeat(64));
    let docker = scenario.fixture.docker.clone();
    let state = scenario.state.path().to_path_buf();
    let ended = tokio::time::timeout(
        Duration::from_secs(300),
        supervise(docker, plan, &state, std::future::pending()),
    )
    .await;
    let events = scenario.fake().events();
    scenario.finish().await?;
    assert_eq!(ended?, Err(HostError::Docker), "{events:?}");
    let names = minted_names(&events);
    assert!(names.len() > 1, "{events:?}");
    for pair in names.windows(2) {
        let deleted = events
            .iter()
            .position(|event| *event == format!("delete:trusted:{}", pair[0]));
        let next = events
            .iter()
            .position(|event| event.starts_with(&format!("mint:{}", pair[1])));
        assert!(deleted.is_some() && deleted < next, "{pair:?} {events:?}");
    }
    Ok(())
}

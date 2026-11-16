// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use velnor_runner_host::local::supervise;

use super::scenario::Scenario;
use super::support::TestResult;

const JOURNAL: &str = "minted-agents.log";

fn registered(events: &[String]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| event.strip_prefix("registered:"))
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn local_host_killed_while_a_mint_reply_never_arrives_deletes_the_registered_agent_on_restart()
-> TestResult<()> {
    let scenario = Scenario::new().await?;
    scenario.fake().stall_mint.store(true, Ordering::SeqCst);
    let state = scenario.state.path().to_path_buf();
    let killed = {
        let docker = scenario.fixture.docker.clone();
        let plan = scenario.plan(1, 0)?;
        let state = state.clone();
        tokio::spawn(async move { supervise(docker, plan, &state, std::future::pending()).await })
    };
    let started = Instant::now();
    while registered(&scenario.fake().events()).is_empty()
        && started.elapsed() < Duration::from_secs(60)
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    killed.abort();
    let _joined = killed.await;
    let before = scenario.fake().events();
    let names = registered(&before);
    let journal = std::fs::read_to_string(state.join(JOURNAL)).unwrap_or_default();
    let mode = std::fs::metadata(state.join(JOURNAL))?.permissions().mode() & 0o777;
    scenario.fake().stall_mint.store(false, Ordering::SeqCst);
    let docker = scenario.fixture.docker.clone();
    let plan = scenario.plan(1, 0)?;
    let ended = tokio::time::timeout(
        Duration::from_secs(120),
        supervise(docker, plan, &state, async {}),
    )
    .await;
    let events = scenario.fake().events();
    scenario.finish().await?;
    assert_eq!(names.len(), 1, "{before:?}");
    assert!(journal.contains(&names[0]), "{journal}");
    assert_eq!(mode, 0o600);
    assert!(
        !before.contains(&format!("delete:trusted:{}", names[0])),
        "{before:?}"
    );
    assert_eq!(ended?, Ok(()), "{events:?}");
    assert!(
        events.contains(&format!("delete:trusted:{}", names[0])),
        "{events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn local_sweep_never_deletes_a_name_cut_short_by_a_torn_last_journal_line() -> TestResult<()>
{
    let scenario = Scenario::new().await?;
    let state = scenario.state.path().to_path_buf();
    std::fs::write(
        state.join(JOURNAL),
        "+ trusted whole-name\n+ trusted retired-name\n- trusted retired-name\n+ trusted torn-na",
    )?;
    let docker = scenario.fixture.docker.clone();
    let plan = scenario.plan(1, 0)?;
    let ended = tokio::time::timeout(
        Duration::from_secs(120),
        supervise(docker, plan, &state, async {}),
    )
    .await;
    let events = scenario.fake().events();
    scenario.finish().await?;
    assert_eq!(ended?, Ok(()), "{events:?}");
    let deleted: Vec<&String> = events
        .iter()
        .filter(|event| event.starts_with("delete:"))
        .collect();
    assert_eq!(deleted, ["delete:trusted:whole-name"], "{events:?}");
    Ok(())
}

#[tokio::test]
async fn local_sweep_keeps_a_journaled_name_the_authority_does_not_know_yet() -> TestResult<()> {
    let scenario = Scenario::new().await?;
    let state = scenario.state.path().to_path_buf();
    std::fs::write(
        state.join(JOURNAL),
        "+ trusted pending-name\n+ trusted gone-name\n",
    )?;
    scenario.fake().agents_absent.store(true, Ordering::SeqCst);
    let docker = scenario.fixture.docker.clone();
    let plan = scenario.plan(1, 0)?;
    let ended = tokio::time::timeout(
        Duration::from_secs(120),
        supervise(docker, plan, &state, async {}),
    )
    .await;
    let events = scenario.fake().events();
    let kept = std::fs::read_to_string(state.join(JOURNAL)).unwrap_or_default();
    scenario.finish().await?;
    assert_eq!(ended?, Ok(()), "{events:?}");
    assert!(kept.contains("pending-name"), "{kept}");
    assert!(kept.contains("gone-name"), "{kept}");
    Ok(())
}

#[tokio::test]
async fn local_failed_mint_keeps_its_name_while_the_agent_is_not_visible_yet() -> TestResult<()> {
    let mut scenario = Scenario::new().await?;
    scenario.fake().mint_failures.store(1, Ordering::SeqCst);
    scenario.fake().agents_absent.store(true, Ordering::SeqCst);
    let plan = scenario.plan(1, 0)?;
    scenario.start(plan);
    let started = std::time::Instant::now();
    while !scenario
        .fake()
        .events()
        .iter()
        .any(|event| event.starts_with("delete:"))
        && started.elapsed() < Duration::from_secs(60)
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let stopped = scenario.stop().await?;
    let events = scenario.fake().events();
    let kept = std::fs::read_to_string(scenario.state.path().join(JOURNAL)).unwrap_or_default();
    scenario.finish().await?;
    assert_eq!(stopped, Ok(()), "{events:?}");
    let refused: Vec<&str> = events
        .iter()
        .filter_map(|event| event.strip_prefix("mint-refused:"))
        .collect();
    assert_eq!(refused.len(), 1, "{events:?}");
    assert!(
        kept.contains(&format!("+ trusted {}", refused[0])),
        "{kept}"
    );
    assert!(
        !kept.contains(&format!("- trusted {}", refused[0])),
        "{kept}"
    );
    Ok(())
}

async fn run_until_mint(scenario: &mut Scenario) -> TestResult<Vec<String>> {
    let plan = scenario.plan(1, 0)?;
    scenario.start(plan);
    wait_for_mint(scenario).await
}

async fn wait_for_mint(scenario: &Scenario) -> TestResult<Vec<String>> {
    let started = Instant::now();
    while scenario
        .fake()
        .events()
        .iter()
        .filter(|event| event.starts_with("mint:"))
        .count()
        < 1
        && started.elapsed() < Duration::from_secs(90)
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(scenario.fake().events())
}

#[tokio::test]
async fn local_name_conflict_retires_its_intent_without_deleting_the_other_agent() -> TestResult<()>
{
    let mut scenario = Scenario::new().await?;
    scenario.fake().mint_conflicts.store(1, Ordering::SeqCst);
    let events = run_until_mint(&mut scenario).await?;
    let stopped = scenario.stop().await?;
    let all = scenario.fake().events();
    let kept = std::fs::read_to_string(scenario.state.path().join(JOURNAL)).unwrap_or_default();
    scenario.finish().await?;
    let conflicted = events
        .iter()
        .find_map(|event| event.strip_prefix("mint-conflict:"))
        .expect("a conflict");
    assert_eq!(stopped, Ok(()), "{all:?}");
    assert!(
        !all.iter()
            .any(|event| event.ends_with(conflicted) && event.starts_with("delete")),
        "{all:?}"
    );
    assert!(kept.is_empty(), "{kept}");
    Ok(())
}

#[tokio::test]
async fn local_confirmed_cleanup_of_a_failed_mint_retires_the_intent_without_a_second_delete()
-> TestResult<()> {
    let mut scenario = Scenario::new().await?;
    scenario.fake().mint_cleaned.store(1, Ordering::SeqCst);
    let events = run_until_mint(&mut scenario).await?;
    let stopped = scenario.stop().await?;
    let all = scenario.fake().events();
    let kept = std::fs::read_to_string(scenario.state.path().join(JOURNAL)).unwrap_or_default();
    scenario.finish().await?;
    let cleaned = events
        .iter()
        .find_map(|event| event.strip_prefix("mint-cleaned:"))
        .expect("a cleaned mint");
    assert_eq!(stopped, Ok(()), "{all:?}");
    assert!(
        !all.iter()
            .any(|event| event.ends_with(cleaned) && event.starts_with("delete")),
        "{all:?}"
    );
    assert!(kept.is_empty(), "{kept}");
    Ok(())
}

#[tokio::test]
async fn local_journal_failure_after_a_confirmed_delete_is_retried_not_dropped() -> TestResult<()> {
    let mut scenario = Scenario::new().await?;
    let journal = scenario.state.path().join(JOURNAL);
    *scenario.fake().journal_to_lock.lock().expect("lock") = Some(journal.clone());
    scenario.fake().mint_failures.store(1, Ordering::SeqCst);
    let plan = scenario.plan(1, 0)?;
    scenario.start(plan);
    let started = Instant::now();
    while !scenario
        .fake()
        .events()
        .iter()
        .any(|event| event.starts_with("delete:"))
        && started.elapsed() < Duration::from_secs(60)
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o600))?;
    let events = wait_for_mint(&scenario).await?;
    let stopped = scenario.stop().await?;
    let kept = std::fs::read_to_string(&journal).unwrap_or_default();
    let all = scenario.fake().events();
    scenario.finish().await?;
    let refused = events
        .iter()
        .find_map(|event| event.strip_prefix("mint-refused:"))
        .expect("a refused mint");
    let deletes = all
        .iter()
        .filter(|event| **event == format!("delete:trusted:{refused}"))
        .count();
    assert_eq!(deletes, 1, "{all:?}");
    assert_eq!(stopped, Ok(()), "{events:?}");
    assert!(kept.is_empty(), "{kept}");
    Ok(())
}

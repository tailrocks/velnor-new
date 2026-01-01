use std::time::Duration;

use super::run_after;
use crate::launch::inspect_tests::{DockerStub, hanging, http, journal};
use crate::launch::resource_capacity::JobCapacity;
use crate::launch_test_support::Engine;
use crate::worker::test_resource_budget;
use crate::{IntentState, Journal, Outcome};

const ENGINE_ID: &str = "preflight-engine";
const OTHER_ENGINE_ID: &str = "other-preflight-engine";
const VOLUME: &str = "w0123456789abcdef0123456789abcdef";
const RUNNER_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIND_ID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[tokio::test]
async fn unbound_journal_stops_before_initial_cleanup() -> Result<(), String> {
    reject_before_cleanup("preflight-unbound", None).await
}

#[tokio::test]
async fn mismatched_engine_stops_before_initial_cleanup() -> Result<(), String> {
    reject_before_cleanup("preflight-mismatch", Some(OTHER_ENGINE_ID)).await
}

async fn reject_before_cleanup(label: &str, bound: Option<&str>) -> Result<(), String> {
    let (_scratch, journal) = journal(label).await?;
    if let Some(engine_id) = bound {
        journal
            .bind_engine(engine_id)
            .await
            .map_err(|error| error.to_string())?;
    }
    seed_done_journal(&journal).await?;
    let stub = DockerStub::open(vec![http(200, &docker_info(ENGINE_ID))])?;

    let result = crate::launch::launch_once(
        "",
        "owner",
        "repo",
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
    )
    .await;
    let requests = stub.finish().await?;

    assert!(result.is_err());
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("/info"));
    assert!(!journal.rows().await.map_err(|error| error.to_string())?[0].cleanup_proven);
    Ok(())
}

#[tokio::test]
async fn trusted_inspect_timeout_keeps_preflight_cleanup_available() -> Result<(), String> {
    let (_scratch, journal) = journal("preflight-trusted-timeout").await?;
    journal
        .bind_engine(ENGINE_ID)
        .await
        .map_err(|error| error.to_string())?;
    let engine = Engine::new();
    let done = seed_done_worker(&journal, &engine).await?;
    let (pending, _) = journal
        .begin_launch("pending-capacity-inspect")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker_volume(pending, "wfedcba9876543210fedcba9876543210")
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![
        http(200, &docker_info(ENGINE_ID)),
        http(200, &docker_info(ENGINE_ID)),
        hanging(),
    ])?;

    let capacity = run_after(
        &stub.docker,
        &engine,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_millis(300),
    )
    .await
    .map_err(|error| error.to_string())?;
    drop(stub);

    assert_eq!(capacity, JobCapacity::denied());
    assert_eq!(
        engine.removed().map_err(|error| error.to_string())?,
        vec![RUNNER_ID.to_owned(), DIND_ID.to_owned()]
    );
    assert_eq!(
        engine
            .removed_volumes()
            .map_err(|error| error.to_string())?
            .len(),
        3
    );
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert!(rows.iter().any(|row| row.id == done && row.cleanup_proven));
    assert!(
        rows.iter()
            .any(|row| row.id == pending && row.state == IntentState::Pending)
    );
    Ok(())
}

async fn seed_done_worker(journal: &Journal, engine: &Engine) -> Result<i64, String> {
    let row = seed_done_journal(journal).await?;
    engine
        .plant_owned(VOLUME, "runner", RUNNER_ID, false)
        .map_err(|error| error.to_string())?;
    engine
        .plant_owned(VOLUME, "dind", DIND_ID, false)
        .map_err(|error| error.to_string())?;
    engine
        .plant_worker_volumes(VOLUME)
        .map_err(|error| error.to_string())?;
    Ok(row)
}

async fn seed_done_journal(journal: &Journal) -> Result<i64, String> {
    let (row, _) = journal
        .begin_launch("completed-worker")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker_volume(row, VOLUME)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker(row, Some(RUNNER_ID), Some(DIND_ID))
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    Ok(row)
}

fn docker_info(engine: &str) -> String {
    format!(r#"{{"ID":"{engine}","NCPU":8,"MemTotal":137438953472}}"#)
}

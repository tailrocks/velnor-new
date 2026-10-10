//! Exited runners free one slot. The next assigned job is minted in this session.

use std::sync::Arc;

use crate::launch::{Admit, admission, drive_offer, statistics_blocked};
use crate::launch_harness::{absent, assigned_wait, ctx, open};
use crate::launch_test_support::{Engine, hex, script, seed_done};
use crate::stage::{PairStop, drive};
use crate::{EnsureError, HostError, IntentState, Outcome, Started};

#[tokio::test]
async fn capacity_two_mints_when_one_runner_exits() -> Result<(), String> {
    let (scratch, journal) = open("backfill-a").await?;
    let engine = Engine::new();
    let runner_a = hex(1);
    let dind_a = hex(2);
    let runner_b = hex(3);
    let dind_b = hex(4);
    let volume_a = seed_done(&journal, "m1", &runner_a, &dind_a).await?;
    let volume_b = seed_done(&journal, "m2", &runner_b, &dind_b).await?;
    engine
        .plant_worker_volumes(&volume_a)
        .map_err(|err| err.to_string())?;
    engine
        .plant_worker_volumes(&volume_b)
        .map_err(|err| err.to_string())?;
    engine
        .plant_owned(&volume_a, "runner", &runner_a, false)
        .map_err(|err| err.to_string())?;
    engine
        .plant_owned(&volume_a, "dind", &dind_a, true)
        .map_err(|err| err.to_string())?;
    engine
        .plant_owned(&volume_b, "runner", &runner_b, true)
        .map_err(|err| err.to_string())?;
    engine
        .plant_owned(&volume_b, "dind", &dind_b, true)
        .map_err(|err| err.to_string())?;
    let decision = admission(&engine, &journal, 2, 2, 2, &assigned_wait(9, 2))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Start { stop: true });
    assert_eq!(
        engine.removed().map_err(|err| err.to_string())?,
        vec![runner_a, dind_a]
    );
    assert!(engine.alive(&dind_b).map_err(|err| err.to_string())?);
    assert!(engine.alive(&runner_b).map_err(|err| err.to_string())?);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    let proven = rows.iter().filter(|row| row.cleanup_proven).count();
    assert_eq!(proven, 1);
    let mut calls = script();
    let minted = drive_offer(
        &mut calls,
        &ctx(),
        &assigned_wait(9, 2),
        &journal,
        |_name, _jit, _bind| async {
            Ok(Started {
                dind_id: hex(6),
                runner_id: hex(5),
            })
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    let minted = minted.ok_or_else(|| "missing worker".to_owned())?;
    assert_eq!(minted.runner_id, hex(5));
    assert_eq!(calls.calls, ["jit", "ack"]);
    assert!(engine.alive(&dind_b).map_err(|err| err.to_string())?);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2].docker_id.as_deref(), Some(hex(5).as_str()));
    assert_eq!(rows[2].dind_id.as_deref(), Some(hex(6).as_str()));
    absent(&scratch.file())
}

#[tokio::test]
async fn gone_runner_without_dind_frees_the_slot() -> Result<(), String> {
    let (scratch, journal) = open("backfill-e").await?;
    let engine = Engine::new();
    let gone = hex(31);
    let live = hex(32);
    let mut live_volume = String::new();
    for (subject, runner) in [("m1", gone.as_str()), ("m2", live.as_str())] {
        let (id, _) = journal
            .begin_launch(subject)
            .await
            .map_err(|err| err.to_string())?;
        let volume = format!("w{id}");
        journal
            .bind_worker_volume(id, &volume)
            .await
            .map_err(|err| err.to_string())?;
        journal
            .bind_worker(id, Some(runner), None)
            .await
            .map_err(|err| err.to_string())?;
        journal
            .finish(id, Outcome::Done)
            .await
            .map_err(|err| err.to_string())?;
        engine
            .plant_worker_volumes(&volume)
            .map_err(|err| err.to_string())?;
        if runner == live {
            live_volume = volume;
        }
    }
    engine
        .plant_owned(&live_volume, "runner", &live, true)
        .map_err(|err| err.to_string())?;
    let decision = admission(&engine, &journal, 2, 2, 0, &assigned_wait(9, 2))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Start { stop: false });
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.iter().filter(|row| row.cleanup_proven).count(), 1);
    assert!(
        rows.iter()
            .any(|row| { row.docker_id.as_deref() == Some(live.as_str()) && !row.cleanup_proven })
    );
    assert!(engine.alive(&live).map_err(|err| err.to_string())?);
    absent(&scratch.file())
}

#[tokio::test]
async fn capacity_one_mints_after_the_pair_is_removed() -> Result<(), String> {
    let (scratch, journal) = open("backfill-b").await?;
    let engine = Engine::new();
    let runner = hex(11);
    let dind = hex(12);
    let volume = seed_done(&journal, "m1", &runner, &dind).await?;
    engine
        .plant_worker_volumes(&volume)
        .map_err(|err| err.to_string())?;
    engine
        .plant_owned(&volume, "runner", &runner, false)
        .map_err(|err| err.to_string())?;
    engine
        .plant_owned(&volume, "dind", &dind, true)
        .map_err(|err| err.to_string())?;
    let decision = admission(&engine, &journal, 1, 1, 1, &assigned_wait(2, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Start { stop: true });
    assert_eq!(
        engine.removed().map_err(|err| err.to_string())?,
        vec![runner, dind]
    );
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert!(rows[0].cleanup_proven);
    let mut calls = script();
    let minted = drive_offer(
        &mut calls,
        &ctx(),
        &assigned_wait(2, 1),
        &journal,
        |_name, _jit, _bind| async {
            Ok(Started {
                dind_id: hex(14),
                runner_id: hex(13),
            })
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert!(minted.is_some());
    assert_eq!(calls.calls, ["jit", "ack"]);
    absent(&scratch.file())
}

#[tokio::test]
async fn uncertain_dind_only_recovery_keeps_reservation_and_owned_resources() -> Result<(), String>
{
    let (scratch, journal) = open("backfill-c").await?;
    let engine = Arc::new(Engine::new());
    let foreign = hex(99);
    engine.foreign(&foreign).map_err(|err| err.to_string())?;
    let mut calls = script();
    let error = drive_offer(
        &mut calls,
        &ctx(),
        &assigned_wait(4, 1),
        &journal,
        |volume, jit, bind| {
            let engine = Arc::clone(&engine);
            let volume = volume.to_owned();
            let jit = jit.to_vec();
            async move {
                let partial = drive(
                    &*engine,
                    &volume,
                    crate::worker::test_resource_budget()?,
                    &jit,
                    PairStop::DindCreated,
                    &bind,
                )
                .await?;
                assert!(partial.runner_id.is_none());
                assert!(partial.dind_id.is_some());
                Err(HostError::Docker)
            }
        },
    )
    .await;
    assert_eq!(error, Err(EnsureError::Uncertain));
    assert_eq!(calls.calls, ["jit"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert!(rows[0].dind_id.is_some());
    assert_eq!(rows[0].docker_id, None);
    let dind = rows[0].dind_id.clone().ok_or_else(|| "dind".to_owned())?;
    let volume = rows[0]
        .worker_volume
        .clone()
        .ok_or_else(|| "worker volume".to_owned())?;
    let decision = admission(&*engine, &journal, 1, 1, 0, &assigned_wait(5, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Hold);
    assert_eq!(
        engine.removed().map_err(|err| err.to_string())?,
        [] as [std::string::String; 0]
    );
    assert_eq!(
        engine.removed_volumes().map_err(|err| err.to_string())?,
        [] as [std::string::String; 0]
    );
    assert!(engine.alive(&foreign).map_err(|err| err.to_string())?);
    assert!(engine.alive(&dind).map_err(|err| err.to_string())?);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert!(!rows[0].cleanup_proven);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].worker_volume.as_deref(), Some(volume.as_str()));
    absent(&scratch.file())
}

#[tokio::test]
async fn replay_of_a_running_worker_does_not_mint_again() -> Result<(), String> {
    let (scratch, journal) = open("backfill-d").await?;
    let engine = Engine::new();
    let runner = hex(21);
    let dind = hex(22);
    let volume = seed_done(&journal, "m7", &runner, &dind).await?;
    engine
        .plant_worker_volumes(&volume)
        .map_err(|err| err.to_string())?;
    engine
        .plant_owned(&volume, "runner", &runner, true)
        .map_err(|err| err.to_string())?;
    engine
        .plant_owned(&volume, "dind", &dind, true)
        .map_err(|err| err.to_string())?;
    assert!(statistics_blocked(1, 1, 2, 1));
    let decision = admission(&engine, &journal, 2, 2, 0, &assigned_wait(8, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Ack { stop: false });
    assert_eq!(
        engine.removed().map_err(|err| err.to_string())?,
        [] as [std::string::String; 0]
    );
    let mut calls = script();
    let again = drive_offer(
        &mut calls,
        &ctx(),
        &assigned_wait(7, 1),
        &journal,
        |_name, _jit, _bind| async {
            Ok(Started {
                dind_id: hex(24),
                runner_id: hex(23),
            })
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    assert_eq!(again, None);
    assert_eq!(calls.calls, ["ack"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].docker_id.as_deref(), Some(runner.as_str()));
    assert!(engine.alive(&dind).map_err(|err| err.to_string())?);
    absent(&scratch.file())
}

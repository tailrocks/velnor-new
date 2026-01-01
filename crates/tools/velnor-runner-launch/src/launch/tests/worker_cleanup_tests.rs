//! Recovery owns the complete per-generation runner, `DinD`, and volume set.

use crate::launch::fakes::Engine;
use crate::launch::harness::{absent, assigned_wait, open};
use crate::launch::{Admit, admission};
use velnor_runner_docker_spec::runner_plan;
use velnor_runner_host::stage::PairEngine;
use velnor_runner_host::worker::{dind_create, join_dind_net, runner_create};
use velnor_runner_host::{IntentState, Journal, Outcome};

#[tokio::test]
async fn pending_and_uncertain_rows_keep_owned_volumes_without_remote_settlement()
-> Result<(), String> {
    unresolved_volume_row_keeps_occupancy("pending-volume", false).await?;
    unresolved_volume_row_keeps_occupancy("uncertain-volume", true).await
}

async fn unresolved_volume_row_keeps_occupancy(label: &str, uncertain: bool) -> Result<(), String> {
    let (scratch, journal) = open(label).await?;
    let engine = Engine::new();
    let (row, fresh) = journal
        .begin_launch("remote-operation")
        .await
        .map_err(|err| err.to_string())?;
    assert!(fresh);
    let volume = velnor_runner_host::worker::new_worker_volume().map_err(|err| err.to_string())?;
    journal
        .bind_worker_volume(row, &volume)
        .await
        .map_err(|err| err.to_string())?;
    engine
        .prepare_volumes(&volume)
        .await
        .map_err(|err| err.to_string())?;
    if uncertain {
        journal
            .finish(row, Outcome::Uncertain)
            .await
            .map_err(|err| err.to_string())?;
    }

    let decision = admission(&engine, &journal, 1, 1, 0, &assigned_wait(15, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Hold);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].state,
        if uncertain {
            IntentState::Uncertain
        } else {
            IntentState::Pending
        }
    );
    assert_eq!(rows[0].worker_volume.as_deref(), Some(volume.as_str()));
    assert!(!rows[0].cleanup_proven);
    assert_eq!(
        engine.removed().map_err(|err| err.to_string())?,
        Vec::<String>::new()
    );
    assert_eq!(
        engine.removed_volumes().map_err(|err| err.to_string())?,
        Vec::<String>::new()
    );
    absent(&scratch.file())
}

#[tokio::test]
async fn completed_worker_exit_keeps_slot_until_checkpointed_cleanup_exists() -> Result<(), String>
{
    let (scratch, journal) = open("worker-crash").await?;
    let engine = Engine::new();
    let (row, volume, dind, runner) = create_unbound_pair(&journal, &engine, "m9r73").await?;

    let occupied = admission(&engine, &journal, 1, 1, 0, &assigned_wait(9, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(occupied, Admit::Hold);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].docker_id.is_none());
    assert!(rows[0].dind_id.is_none());
    assert_eq!(rows[0].state, IntentState::Done);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(
        engine.removed().map_err(|err| err.to_string())?,
        Vec::<String>::new()
    );

    engine
        .set_running(&runner, false)
        .map_err(|err| err.to_string())?;
    let admitted = admission(&engine, &journal, 1, 1, 0, &assigned_wait(10, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(admitted, Admit::Hold);
    assert_eq!(
        engine.removed().map_err(|err| err.to_string())?,
        Vec::<String>::new()
    );
    assert_eq!(
        engine.removed_volumes().map_err(|err| err.to_string())?,
        Vec::<String>::new()
    );
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, row);
    assert!(!rows[0].cleanup_proven);
    assert!(engine.alive(&runner).map_err(|err| err.to_string())?);
    assert!(engine.alive(&dind).map_err(|err| err.to_string())?);
    assert_eq!(rows[0].worker_volume.as_deref(), Some(volume.as_str()));
    absent(&scratch.file())
}

async fn create_unbound_pair(
    journal: &Journal,
    engine: &Engine,
    subject: &str,
) -> Result<(i64, String, String, String), String> {
    let (row, fresh) = journal
        .begin_launch(subject)
        .await
        .map_err(|err| err.to_string())?;
    assert!(fresh);
    let volume = velnor_runner_host::worker::new_worker_volume().map_err(|err| err.to_string())?;
    journal
        .bind_worker_volume(row, &volume)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(row, Outcome::Done)
        .await
        .map_err(|err| err.to_string())?;
    engine
        .prepare_volumes(&volume)
        .await
        .map_err(|err| err.to_string())?;
    let dind = engine
        .create(&dind_create(&volume).map_err(|err| err.to_string())?)
        .await
        .map_err(|err| err.to_string())?;
    engine.start(&dind).await.map_err(|err| err.to_string())?;
    let runner = runner_create(&runner_plan(&volume).map_err(|err| err.to_string())?)
        .map_err(|err| err.to_string())?;
    let runner = join_dind_net(runner, &dind).map_err(|err| err.to_string())?;
    let runner = engine
        .create(&runner)
        .await
        .map_err(|err| err.to_string())?;
    engine.start(&runner).await.map_err(|err| err.to_string())?;
    Ok((row, volume, dind, runner))
}

#[tokio::test]
async fn independent_journals_never_delete_without_checkpointed_cleanup() -> Result<(), String> {
    let (scratch_a, journal_a) = open("identity-a").await?;
    let (scratch_b, journal_b) = open("identity-b").await?;
    let engine = Engine::new();
    let (row_a, volume_a, dind_a, runner_a) =
        create_unbound_pair(&journal_a, &engine, "offer-a").await?;
    let (row_b, volume_b, dind_b, runner_b) =
        create_unbound_pair(&journal_b, &engine, "offer-b").await?;
    assert_eq!(row_a, row_b);
    assert_ne!(volume_a, volume_b);
    engine
        .set_running(&runner_a, false)
        .map_err(|err| err.to_string())?;

    let held_a = admission(&engine, &journal_a, 1, 1, 0, &assigned_wait(12, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(held_a, Admit::Hold);
    assert_eq!(
        engine.removed().map_err(|err| err.to_string())?,
        Vec::<String>::new()
    );
    assert!(engine.alive(&runner_b).map_err(|err| err.to_string())?);
    assert!(engine.alive(&dind_b).map_err(|err| err.to_string())?);
    assert!(engine.alive(&dind_a).map_err(|err| err.to_string())?);
    assert_eq!(
        engine.removed_volumes().map_err(|err| err.to_string())?,
        Vec::<String>::new()
    );

    let retained = admission(&engine, &journal_b, 1, 1, 0, &assigned_wait(13, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(retained, Admit::Hold);
    let recovered_rows = journal_b.rows().await.map_err(|err| err.to_string())?;
    assert!(recovered_rows[0].docker_id.is_none());
    assert!(recovered_rows[0].dind_id.is_none());
    assert!(!recovered_rows[0].cleanup_proven);
    let journal_a_rows = journal_a.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(
        journal_a_rows[0].worker_volume.as_deref(),
        Some(volume_a.as_str())
    );
    assert!(!journal_a_rows[0].cleanup_proven);
    absent(&scratch_a.file())?;
    absent(&scratch_b.file())
}

#[tokio::test]
async fn foreign_container_at_owned_name_is_not_adopted_or_removed_without_cleanup_proof()
-> Result<(), String> {
    let (scratch, journal) = open("foreign-worker-name").await?;
    let engine = Engine::new();
    let (row, fresh) = journal
        .begin_launch("offer-foreign")
        .await
        .map_err(|err| err.to_string())?;
    assert!(fresh);
    let volume = velnor_runner_host::worker::new_worker_volume().map_err(|err| err.to_string())?;
    journal
        .bind_worker_volume(row, &volume)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(row, Outcome::Done)
        .await
        .map_err(|err| err.to_string())?;
    engine
        .prepare_volumes(&volume)
        .await
        .map_err(|err| err.to_string())?;
    let foreign = crate::launch::fakes::hex(77);
    engine
        .foreign_named_worker(&volume, "runner", &foreign, false)
        .map_err(|err| err.to_string())?;

    let result = admission(&engine, &journal, 1, 1, 0, &assigned_wait(14, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(result, Admit::Hold);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert!(rows[0].docker_id.is_none());
    assert!(!rows[0].cleanup_proven);
    assert_eq!(
        engine.removed().map_err(|err| err.to_string())?,
        Vec::<String>::new()
    );
    assert!(engine.alive(&foreign).map_err(|err| err.to_string())?);
    absent(&scratch.file())
}

#[tokio::test]
async fn foreign_volume_prevents_cleanup_proof_and_keeps_the_slot() -> Result<(), String> {
    let (scratch, journal) = open("foreign-worker-volume").await?;
    let engine = Engine::new();
    let (_, volume, _, runner) = create_unbound_pair(&journal, &engine, "m9r74").await?;
    engine
        .set_running(&runner, false)
        .map_err(|err| err.to_string())?;
    engine
        .foreign_volume(&format!("{volume}-docker"))
        .map_err(|err| err.to_string())?;

    let decision = admission(&engine, &journal, 1, 1, 0, &assigned_wait(11, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Hold);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert!(!rows[0].cleanup_proven);
    assert_eq!(
        engine.removed_volumes().map_err(|err| err.to_string())?,
        Vec::<String>::new()
    );
    absent(&scratch.file())
}

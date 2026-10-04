//! Exited runners free one slot. The next assigned job is minted in this session.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::launch::{Admit, admission, drive_offer, statistics_blocked};
use crate::launch_harness::{Mode, Script, absent, assigned_wait, ctx, open};
use crate::stage::{PairEngine, PairStop, drive};
use crate::worker::CreateProjection;
use crate::{EnsureError, HostError, IntentState, Journal, Outcome, Started};

fn hex(n: u64) -> String {
    format!("{n:064x}")
}

struct Engine {
    ids: Mutex<Vec<String>>,
    running: Mutex<HashMap<String, bool>>,
    names: Mutex<HashMap<String, String>>,
    removed: Mutex<Vec<String>>,
    next: Mutex<u64>,
}

impl Engine {
    fn new() -> Self {
        Self {
            ids: Mutex::new(Vec::new()),
            running: Mutex::new(HashMap::new()),
            names: Mutex::new(HashMap::new()),
            removed: Mutex::new(Vec::new()),
            next: Mutex::new(1),
        }
    }

    fn plant(&self, id: &str, up: bool) -> Result<(), HostError> {
        self.ids
            .lock()
            .map_err(|_| HostError::Docker)?
            .push(id.to_owned());
        self.running
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(id.to_owned(), up);
        Ok(())
    }

    fn foreign(&self, id: &str) -> Result<(), HostError> {
        self.names
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(id.to_owned(), id.to_owned());
        self.plant(id, true)
    }

    fn removed(&self) -> Result<Vec<String>, HostError> {
        self.removed
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| HostError::Docker)
    }

    fn alive(&self, id: &str) -> Result<bool, HostError> {
        Ok(self
            .ids
            .lock()
            .map_err(|_| HostError::Docker)?
            .iter()
            .any(|kept| kept == id))
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "fake engine matches the async trait and does not await"
)]
impl PairEngine for Engine {
    async fn prepare_volumes(&self, _volume: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn create(&self, _spec: &CreateProjection) -> Result<String, HostError> {
        let mut next = self.next.lock().map_err(|_| HostError::Docker)?;
        let id = hex(*next);
        *next = next.saturating_add(1);
        drop(next);
        self.plant(&id, false)?;
        Ok(id)
    }

    async fn start(&self, id: &str) -> Result<(), HostError> {
        self.running
            .lock()
            .map_err(|_| HostError::Docker)?
            .insert(id.to_owned(), true);
        Ok(())
    }

    async fn write_jit(&self, _id: &str, _jit: &[u8]) -> Result<(), HostError> {
        Ok(())
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        self.removed
            .lock()
            .map_err(|_| HostError::Docker)?
            .push(id.to_owned());
        self.ids
            .lock()
            .map_err(|_| HostError::Docker)?
            .retain(|kept| kept != id);
        self.running
            .lock()
            .map_err(|_| HostError::Docker)?
            .remove(id);
        Ok(())
    }

    async fn id_for_name(&self, name: &str) -> Result<Option<String>, HostError> {
        let known = self
            .ids
            .lock()
            .map_err(|_| HostError::Docker)?
            .iter()
            .any(|id| id == name);
        if known {
            return Ok(Some(name.to_owned()));
        }
        let names = self.names.lock().map_err(|_| HostError::Docker)?;
        Ok(names.get(name).cloned())
    }

    async fn running(&self, id: &str) -> Result<bool, HostError> {
        Ok(self
            .running
            .lock()
            .map_err(|_| HostError::Docker)?
            .get(id)
            .copied()
            .unwrap_or(false))
    }
}

async fn seed_done(
    journal: &Journal,
    subject: &str,
    runner: &str,
    dind: &str,
) -> Result<(), String> {
    let id = journal
        .begin("launch", subject)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind_worker(id, Some(runner), Some(dind))
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind_worker(id, Some(runner), Some(dind))
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Done)
        .await
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn script() -> Script {
    Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    }
}

#[tokio::test]
async fn capacity_two_mints_when_one_runner_exits() -> Result<(), String> {
    let (scratch, journal) = open("backfill-a").await?;
    let engine = Engine::new();
    let runner_a = hex(1);
    let dind_a = hex(2);
    let runner_b = hex(3);
    let dind_b = hex(4);
    seed_done(&journal, "m1", &runner_a, &dind_a).await?;
    seed_done(&journal, "m2", &runner_b, &dind_b).await?;
    engine
        .plant(&runner_a, false)
        .map_err(|err| err.to_string())?;
    engine.plant(&dind_a, true).map_err(|err| err.to_string())?;
    engine
        .plant(&runner_b, true)
        .map_err(|err| err.to_string())?;
    engine.plant(&dind_b, true).map_err(|err| err.to_string())?;
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
    for (subject, runner) in [("m1", gone.as_str()), ("m2", live.as_str())] {
        let id = journal
            .begin("launch", subject)
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
    }
    engine.plant(&live, true).map_err(|err| err.to_string())?;
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
    seed_done(&journal, "m1", &runner, &dind).await?;
    engine
        .plant(&runner, false)
        .map_err(|err| err.to_string())?;
    engine.plant(&dind, true).map_err(|err| err.to_string())?;
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
async fn uncertain_dind_keeps_the_slot_and_the_foreign_container() -> Result<(), String> {
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
        |_name, jit, bind| {
            let engine = Arc::clone(&engine);
            let jit = jit.to_vec();
            async move {
                let partial = drive(&*engine, "m4", &jit, PairStop::DindCreated, &bind).await?;
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
    let decision = admission(&*engine, &journal, 1, 1, 0, &assigned_wait(5, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Hold);
    let removed = engine.removed().map_err(|err| err.to_string())?;
    assert!(removed.is_empty());
    assert!(engine.alive(&foreign).map_err(|err| err.to_string())?);
    assert!(engine.alive(&dind).map_err(|err| err.to_string())?);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert!(!rows[0].cleanup_proven);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    absent(&scratch.file())
}

#[tokio::test]
async fn replay_of_a_running_worker_does_not_mint_again() -> Result<(), String> {
    let (scratch, journal) = open("backfill-d").await?;
    let engine = Engine::new();
    let runner = hex(21);
    let dind = hex(22);
    seed_done(&journal, "m7", &runner, &dind).await?;
    engine.plant(&runner, true).map_err(|err| err.to_string())?;
    engine.plant(&dind, true).map_err(|err| err.to_string())?;
    assert!(statistics_blocked(1, 1, 2, 1));
    let decision = admission(&engine, &journal, 2, 2, 0, &assigned_wait(8, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Ack { stop: false });
    assert!(engine.removed().map_err(|err| err.to_string())?.is_empty());
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

//! A launch row without worker IDs stays occupied until cleanup is proven.

use crate::launch::{Admit, admission};
use crate::launch_harness::{
    absent, assigned_wait, available, open, started_progress, started_wait,
};
use crate::stage::PairEngine;
use crate::worker::{CreateProjection, WorkerVolumeRole, WorkerVolumeVerification};
use crate::{HostError, Journal, Outcome};

struct Idle;

#[expect(
    clippy::unused_async_trait_impl,
    reason = "the trait is async and this engine never awaits"
)]
impl PairEngine for Idle {
    async fn prepare_volumes(&self, _volume: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn create(&self, _spec: &CreateProjection) -> Result<String, HostError> {
        Ok(String::new())
    }

    async fn start(&self, _id: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn write_jit(&self, _id: &str, _jit: &[u8]) -> Result<(), HostError> {
        Ok(())
    }

    async fn remove(&self, _id: &str) -> Result<(), HostError> {
        Ok(())
    }

    async fn id_for_name(&self, _name: &str) -> Result<Option<String>, HostError> {
        Ok(None)
    }

    async fn worker_id_for_name(
        &self,
        _name: &str,
        _volume: &str,
        _role: &str,
    ) -> Result<Option<String>, HostError> {
        Ok(None)
    }

    async fn verify_volume(
        &self,
        _worker: &str,
        _role: WorkerVolumeRole,
    ) -> Result<WorkerVolumeVerification, HostError> {
        Ok(WorkerVolumeVerification::Absent)
    }

    async fn remove_worker_volumes(&self, _volume: &str) -> Result<bool, HostError> {
        Ok(true)
    }

    async fn running(&self, _id: &str) -> Result<bool, HostError> {
        Ok(false)
    }
}

async fn uncertain_without_ids(journal: &Journal, subject: &str) -> Result<(), String> {
    let id = journal
        .begin("launch", subject)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())
}

#[tokio::test]
async fn idless_uncertain_rows_keep_both_slots() -> Result<(), String> {
    let (scratch, journal) = open("idless").await?;
    uncertain_without_ids(&journal, "m1").await?;
    uncertain_without_ids(&journal, "m2").await?;
    let decision = admission(&Idle, &journal, 2, 2, 0, &assigned_wait(9, 2))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Hold);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| !row.cleanup_proven));
    absent(&scratch.file())
}
#[tokio::test]
async fn own_idless_uncertain_row_does_not_block_its_mint() -> Result<(), String> {
    let (scratch, journal) = open("idless-self-launch").await?;
    uncertain_without_ids(&journal, "m4r41").await?;
    let decision = admission(&Idle, &journal, 1, 1, 0, &available(&[41]))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Start { stop: true });
    absent(&scratch.file())
}

#[tokio::test]
async fn own_idless_uncertain_row_keeps_its_reservation() -> Result<(), String> {
    let (scratch, journal) = open("idless-self").await?;
    let id = journal
        .begin("launch", "m9")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;
    let decision = admission(&Idle, &journal, 1, 1, 0, &assigned_wait(9, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Hold);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].state, crate::IntentState::Uncertain);
    assert!(!rows[0].cleanup_proven);
    absent(&scratch.file())
}

#[tokio::test]
async fn other_idless_uncertain_row_still_blocks_mint() -> Result<(), String> {
    let (scratch, journal) = open("idless-other").await?;
    uncertain_without_ids(&journal, "m8").await?;
    let decision = admission(&Idle, &journal, 1, 1, 0, &assigned_wait(9, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Hold);
    absent(&scratch.file())
}

#[tokio::test]
async fn partial_uncertain_row_still_blocks_its_mint() -> Result<(), String> {
    let (scratch, journal) = open("partial-self").await?;
    let id = journal
        .begin("launch", "m9")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind_worker(id, None, Some(&hex(1)))
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;
    let decision = admission(&Idle, &journal, 1, 1, 0, &assigned_wait(9, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Hold);
    absent(&scratch.file())
}

fn hex(n: u64) -> String {
    format!("{n:064x}")
}

#[tokio::test]
async fn full_slot_acks_started_progress() -> Result<(), String> {
    let (scratch, journal) = open("progress-full").await?;
    uncertain_without_ids(&journal, "m8").await?;
    let decision = admission(&Idle, &journal, 1, 1, 0, &started_progress(11, 5))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Ack { stop: false });
    absent(&scratch.file())
}

#[tokio::test]
async fn free_slot_still_scales_started_progress() -> Result<(), String> {
    let (scratch, journal) = open("progress-free").await?;
    let decision = admission(&Idle, &journal, 2, 2, 0, &started_progress(11, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Start { stop: false });
    absent(&scratch.file())
}

#[tokio::test]
async fn failed_row_with_partial_pair_keeps_its_slot() -> Result<(), String> {
    let (scratch, journal) = open("failed-pair").await?;
    let id = journal
        .begin("launch", "m7r61")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind_worker(id, Some("runner-7"), Some("dind-7"))
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::DefiniteFailure)
        .await
        .map_err(|err| err.to_string())?;

    let decision = admission(&Idle, &journal, 1, 1, 0, &assigned_wait(9, 1))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Hold);
    assert!(
        journal
            .rows()
            .await
            .map_err(|err| err.to_string())?
            .iter()
            .any(|row| row.id == id && !row.cleanup_proven)
    );
    absent(&scratch.file())
}

#[tokio::test]
async fn full_slot_started_notice_is_acknowledged() -> Result<(), String> {
    let (scratch, journal) = open("progress-full").await?;
    let id = journal
        .begin("launch", "m8")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind_worker(id, None, Some(&hex(1)))
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;
    let decision = admission(&Idle, &journal, 1, 1, 0, &started_wait(9, 5))
        .await
        .map_err(|err| err.to_string())?;
    assert_eq!(decision, Admit::Ack { stop: false });
    absent(&scratch.file())
}

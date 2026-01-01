use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{HostError, Journal, Outcome};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-worker-volume-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let result = std::fs::remove_dir_all(&self.0);
        let _ignored = result.err().map(|error| error.kind());
    }
}

#[tokio::test]
async fn worker_volume_is_durable_and_cannot_be_rebound() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = journal.begin("launch", "m1").await?;
    journal.bind_worker_volume(id, "w1").await?;
    journal.bind_worker_volume(id, "w1").await?;
    assert_eq!(
        journal.bind_worker_volume(id, "w2").await,
        Err(HostError::Journal)
    );

    let row = Journal::open(&scratch.file())
        .await?
        .rows()
        .await?
        .remove(0);
    assert_eq!(row.worker_volume.as_deref(), Some("w1"));
    Ok(())
}

#[tokio::test]
async fn recovered_worker_ids_are_immutable() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = journal.begin("launch", "offer").await?;
    journal.bind_worker(id, None, Some("dind-a")).await?;
    journal.bind_worker(id, Some("runner-a"), None).await?;
    journal
        .bind_worker(id, Some("runner-a"), Some("dind-a"))
        .await?;
    assert_eq!(
        journal.bind_worker(id, Some("runner-b"), None).await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.bind_worker(id, None, Some("dind-b")).await,
        Err(HostError::Journal)
    );
    let rows = journal.rows().await?;
    assert_eq!(rows[0].docker_id.as_deref(), Some("runner-a"));
    assert_eq!(rows[0].dind_id.as_deref(), Some("dind-a"));
    Ok(())
}

#[tokio::test]
async fn launch_replay_keeps_original_row_until_physical_proof() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    let old = journal.begin("launch", "same-offer").await?;
    journal.finish(old, Outcome::Done).await?;
    assert!(journal.record_cleanup(old).await.is_err());

    let replay = journal.begin("launch", "same-offer").await?;
    assert_eq!(replay, old);
    let rows = Journal::open(&scratch.file()).await?.rows().await?;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(rows[0].state, crate::IntentState::Done);
    Ok(())
}

#[tokio::test]
async fn generic_launch_begin_reuses_failed_may_have_effect_after_reopen() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let row = journal.begin("launch", "ambiguous-offer").await?;
    journal.record_launch_effect_intent(row).await?;
    journal.finish(row, Outcome::Uncertain).await?;
    journal.finish(row, Outcome::DefiniteFailure).await?;
    drop(journal);

    let reopened = Journal::open(&path).await?;
    let persisted = reopened
        .rows()
        .await?
        .into_iter()
        .find(|candidate| candidate.id == row)
        .ok_or(HostError::Journal)?;
    assert_eq!(persisted.state, crate::IntentState::Failed);
    assert_eq!(
        persisted.launch_effect,
        crate::LaunchEffectState::MayHaveEffect
    );
    assert_eq!(reopened.begin("launch", "ambiguous-offer").await?, row);
    assert_eq!(reopened.rows().await?.len(), 1);
    Ok(())
}

//! Durable launch identity and slot reservations.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{HostError, IntentState, Journal, Outcome};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-launch-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

async fn open(path: &Path) -> Result<Journal, HostError> {
    Journal::open(path).await
}

#[tokio::test]
async fn assignment_key_survives_queue_redelivery_and_separates_sets() -> Result<(), HostError> {
    let scratch = Scratch::new("assignment")?;
    let journal = open(&scratch.file()).await?;
    let first = journal.begin_assignment(7, 42, 100).await?;
    assert_eq!(journal.begin_assignment(7, 42, 101).await?, first);
    assert_ne!(journal.begin_assignment(8, 42, 101).await?, first);
    assert_eq!(journal.rows().await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn legacy_assignment_is_adopted_without_replacing_its_launch_identity()
-> Result<(), HostError> {
    let scratch = Scratch::new("legacy")?;
    let journal = open(&scratch.file()).await?;
    let legacy = journal.begin("launch", "m100r42").await?;
    let legacy_launch_id = journal.rows().await?[0].launch_id.clone();
    assert_eq!(journal.begin_assignment(7, 42, 100).await?, legacy);
    let row = journal.rows().await?.remove(0);
    assert_eq!(row.launch_id, legacy_launch_id);
    assert_eq!(row.assignment_key.as_deref(), Some("7:42"));
    Ok(())
}

#[tokio::test]
async fn uncertain_launch_reserves_slot_until_cleanup_is_proven() -> Result<(), HostError> {
    let scratch = Scratch::new("reservation")?;
    let journal = open(&scratch.file()).await?;
    let id = journal.begin_assignment(7, 42, 100).await?;
    assert_eq!(journal.occupied_launches().await?, 1);
    journal.finish(id, Outcome::Uncertain).await?;
    assert_eq!(journal.occupied_launches().await?, 1);
    let row = journal.rows().await?.remove(0);
    assert_eq!(row.state, IntentState::Uncertain);
    assert!(row.docker_id.is_none());
    assert!(row.dind_id.is_none());
    journal
        .bind_pair(id, "runner-container", "dind-container")
        .await?;
    assert_eq!(
        journal
            .bind_pair(id, "other-runner", "dind-container")
            .await,
        Err(HostError::Journal)
    );
    assert_eq!(journal.occupied_launches().await?, 1);
    journal.record_cleanup(id).await?;
    assert_eq!(journal.occupied_launches().await?, 0);
    Ok(())
}

#[tokio::test]
async fn launch_identity_and_seed_generation_are_immutable() -> Result<(), HostError> {
    let scratch = Scratch::new("identity")?;
    let journal = open(&scratch.file()).await?;
    journal.bind_engine("docker-engine-a").await?;
    assert_eq!(
        journal.bind_engine("docker-engine-b").await,
        Err(HostError::Journal)
    );
    let id = journal.begin("launch", "assignment").await?;
    let identity = journal.launch_identity(id).await?;
    assert_eq!(identity.intent_id(), id);
    assert_eq!(identity.engine_id(), "docker-engine-a");
    assert_eq!(
        identity.private_volume(),
        format!("v{}", identity.launch_id())
    );
    journal
        .bind_seed_generation(id, "generation-sha256-a")
        .await?;
    assert_eq!(
        journal
            .bind_seed_generation(id, "generation-sha256-b")
            .await,
        Err(HostError::Journal)
    );
    Ok(())
}

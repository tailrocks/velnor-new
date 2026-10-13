//! Occupancy and revision behavior for launch rows.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{HostError, Journal, Outcome};

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
async fn revision_advances_with_committed_launch_changes() -> Result<(), HostError> {
    let scratch = Scratch::new("revision")?;
    let journal = open(&scratch.file()).await?;
    let initial = journal.revision().await?;
    journal.bind_engine("docker-engine-a").await?;
    let after_engine = journal.revision().await?;
    assert!(after_engine > initial);
    let id = journal.begin("launch", "revision-subject").await?;
    let after_begin = journal.revision().await?;
    assert!(after_begin > after_engine);
    journal.finish(id, Outcome::Uncertain).await?;
    let after_finish = journal.revision().await?;
    assert!(after_finish > after_begin);
    journal.record_cleanup(id).await?;
    assert!(journal.revision().await? > after_finish);
    Ok(())
}

#[tokio::test]
async fn acknowledgment_does_not_release_a_worker_slot() -> Result<(), HostError> {
    let scratch = Scratch::new("ack-slot")?;
    let journal = open(&scratch.file()).await?;
    let id = journal.begin("launch", "ack-slot-subject").await?;
    journal.finish(id, Outcome::Done).await?;
    assert_eq!(journal.occupied_launches().await?, 1);
    journal.record_cleanup(id).await?;
    assert_eq!(journal.occupied_launches().await?, 0);
    Ok(())
}

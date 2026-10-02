//! Journal durability. Each call commits before it returns.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{HostError, IntentState, Journal, Outcome};

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-host-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self { path })
    }

    fn file(&self) -> PathBuf {
        self.path.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let cleanup = std::fs::remove_dir_all(&self.path);
        let _kept = cleanup.err().map(|err| err.kind());
    }
}

async fn open(path: &Path) -> Result<Journal, HostError> {
    Journal::open(path).await
}

#[tokio::test]
async fn reopen_sees_the_row() -> Result<(), HostError> {
    let scratch = Scratch::new("reopen")?;
    let path = scratch.file();
    let id = {
        let journal = open(&path).await?;
        journal.begin("provision").await?
    };
    let again = open(&path).await?;
    assert_eq!(again.read(id).await?, IntentState::Pending);
    Ok(())
}

#[tokio::test]
async fn uncertain_outcome_keeps_the_row() -> Result<(), HostError> {
    let scratch = Scratch::new("uncertain")?;
    let journal = open(&scratch.file()).await?;
    let held = journal.begin("acquire").await?;
    journal.finish(held, Outcome::Uncertain).await?;
    let next = journal.begin("provision").await?;
    assert_eq!(journal.read(held).await?, IntentState::Uncertain);
    assert_eq!(journal.read(next).await?, IntentState::Pending);
    drop(journal);
    let again = open(&scratch.file()).await?;
    assert_eq!(again.read(held).await?, IntentState::Uncertain);
    Ok(())
}

#[tokio::test]
async fn empty_and_quoted_kinds_are_rejected() -> Result<(), HostError> {
    let scratch = Scratch::new("kind")?;
    let journal = open(&scratch.file()).await?;
    assert_eq!(journal.begin("").await, Err(HostError::Journal));
    assert_eq!(journal.begin("it's").await, Err(HostError::Journal));
    assert_eq!(journal.begin("say \"hi\"").await, Err(HostError::Journal));
    let id = journal.begin("provision").await?;
    assert_eq!(journal.read(id).await?, IntentState::Pending);
    Ok(())
}

#[tokio::test]
async fn drop_does_not_delete_pending_rows() -> Result<(), HostError> {
    let scratch = Scratch::new("drop")?;
    let path = scratch.file();
    let id = {
        let journal = open(&path).await?;
        let id = journal.begin("provision").await?;
        let clone = journal.clone();
        drop(clone);
        drop(journal);
        id
    };
    assert!(path.is_file());
    let again = open(&path).await?;
    assert_eq!(again.read(id).await?, IntentState::Pending);
    Ok(())
}

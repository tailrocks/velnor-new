//! Durability boundary: intents commit before effects and survive
//! reopen; hostile subjects never reach the journal.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_runner_journal::{HostError, IntentState, Journal, Outcome};

fn scratch(label: &str) -> Result<PathBuf, String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("velnor-journal-{label}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

#[tokio::test]
async fn intent_lifecycle_commits_and_survives_reopen() -> Result<(), String> {
    let dir = scratch("lifecycle")?;
    let path = dir.join("journal.db");
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let id = journal
        .begin("launch", "job-1")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal.read(id).await.map_err(|error| error.to_string())?,
        IntentState::Pending
    );
    journal
        .finish(id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);

    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        reopened.read(id).await.map_err(|error| error.to_string())?,
        IntentState::Done
    );
    drop(reopened);
    std::fs::remove_dir_all(dir).map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn hostile_subjects_fail_closed_without_a_row() -> Result<(), String> {
    let dir = scratch("hostile")?;
    let path = dir.join("journal.db");
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(journal.begin("launch", "").await, Err(HostError::Journal));
    assert!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    drop(journal);
    std::fs::remove_dir_all(dir).map_err(|error| error.to_string())?;
    Ok(())
}

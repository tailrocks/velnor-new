//! Reproductions for malformed persisted journal schemas.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{IntentState, Journal, Outcome};

struct SchemaScratch {
    path: PathBuf,
}

impl SchemaScratch {
    fn new() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-malformed-journal-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self { path })
    }

    fn file(&self) -> PathBuf {
        self.path.join("journal.db")
    }
}

impl Drop for SchemaScratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.path);
    }
}

async fn seed_duplicate_ids(path: &Path) -> Result<(), String> {
    let path = path
        .to_str()
        .ok_or_else(|| "journal test path is not UTF-8".to_owned())?;
    let db = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO intents (id, kind, subject, state, cleanup_proven) VALUES (7, 'launch', 'first', 'uncertain', 0), (7, 'launch', 'second', 'uncertain', 0)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute("PRAGMA user_version = 1", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(db);
    Ok(())
}

// Unfinished merged expectation: it asks for an ambiguous multi-row update
// while the journal's one-row write invariant intentionally fails closed.
#[cfg(any())]
#[tokio::test]
async fn duplicate_ids_are_accepted_and_finish_mutates_both_rows() -> Result<(), String> {
    let scratch = SchemaScratch::new()?;
    let path = scratch.file();
    seed_duplicate_ids(&path).await?;
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(
        journal.finish(7, Outcome::DefiniteFailure).await,
        Err(crate::HostError::Journal)
    );
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.state == IntentState::Failed));
    Ok(())
}

#[cfg(any())]
#[tokio::test]
async fn duplicate_ids_are_accepted_and_cleanup_mutates_both_rows() -> Result<(), String> {
    let scratch = SchemaScratch::new()?;
    let path = scratch.file();
    seed_duplicate_ids(&path).await?;
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(
        journal.record_cleanup(7).await,
        Err(crate::HostError::Journal)
    );
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.cleanup_proven));
    Ok(())
}

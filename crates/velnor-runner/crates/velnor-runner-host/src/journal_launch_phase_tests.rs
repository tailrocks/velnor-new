//! Effect phase stays unknown for rows migrated from the old schema.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{HostError, Journal};

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-launch-phase-{}-{n}", std::process::id()));
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
async fn old_schema_migration_keeps_effect_phase_unknown() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let file = scratch.file();
    let text = file.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO intents (kind, subject, state) VALUES ('launch', 'legacy', 'uncertain')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    let journal = open(&scratch.file()).await?;
    let row = journal.rows().await?.remove(0);
    assert_eq!(row.launch_phase, None);
    assert_eq!(row.scale_set_id, None);
    assert_eq!(row.request_id, None);
    assert_eq!(row.runner_name, None);
    assert_eq!(row.docker_engine_id, None);
    assert_eq!(journal.rows().await?.remove(0).launch_phase, None);
    Ok(())
}

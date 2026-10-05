//! Launch identity and effect phase survive journal reopen and migration.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::journal::LaunchIdentity;
use crate::{HostError, Journal, LaunchPhase};

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-launch-phase-{}-{n}",
            std::process::id()
        ));
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

fn identity(runner_name: &str, volume: &str) -> LaunchIdentity {
    LaunchIdentity {
        scale_set_id: 71,
        request_id: Some(9),
        runner_name: runner_name.to_owned(),
        worker_volume: volume.to_owned(),
        docker_engine_id: "engine-test-1".to_owned(),
    }
}

async fn open(path: &Path) -> Result<Journal, HostError> {
    Journal::open(path).await
}

#[tokio::test]
async fn prepared_identity_is_atomic_persistent_and_not_rebound() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = open(&scratch.file()).await?;
    let (id, fresh) = journal
        .begin_prepared_launch("m77r9", &identity("v9", "w0001"))
        .await?;
    assert!(fresh);
    let (same_id, reused) = journal
        .begin_prepared_launch("m77r9", &identity("replacement", "w0002"))
        .await?;
    assert_eq!(same_id, id);
    assert!(!reused);
    let row = journal.rows().await?.remove(0);
    assert_eq!(row.scale_set_id, Some(71));
    assert_eq!(row.request_id, Some(9));
    assert_eq!(row.runner_name.as_deref(), Some("v9"));
    assert_eq!(row.worker_volume.as_deref(), Some("w0001"));
    assert_eq!(row.docker_engine_id.as_deref(), Some("engine-test-1"));
    assert_eq!(row.launch_phase, Some(LaunchPhase::Prepared));
    drop(journal);

    let reopened = open(&scratch.file()).await?;
    let row = reopened.rows().await?.remove(0);
    assert_eq!(row.id, id);
    assert_eq!(row.runner_name.as_deref(), Some("v9"));
    assert_eq!(row.worker_volume.as_deref(), Some("w0001"));
    assert_eq!(row.launch_phase, Some(LaunchPhase::Prepared));
    Ok(())
}

#[tokio::test]
async fn phase_advancement_is_monotone_and_survives_reopen() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = open(&scratch.file()).await?;
    let (id, fresh) = journal
        .begin_prepared_launch("m77r9", &identity("v9", "w0001"))
        .await?;
    assert!(fresh);
    journal
        .advance_launch_phase(id, LaunchPhase::AcquireRequested)
        .await?;
    journal
        .advance_launch_phase(id, LaunchPhase::Acquired)
        .await?;
    journal
        .advance_launch_phase(id, LaunchPhase::JitRequested)
        .await?;
    assert!(matches!(
        journal
            .advance_launch_phase(id, LaunchPhase::Prepared)
            .await,
        Err(HostError::Journal)
    ));
    let row = journal.rows().await?.remove(0);
    assert_eq!(row.launch_phase, Some(LaunchPhase::JitRequested));
    drop(journal);

    let reopened = open(&scratch.file()).await?;
    let row = reopened.rows().await?.remove(0);
    assert_eq!(row.launch_phase, Some(LaunchPhase::JitRequested));
    Ok(())
}

#[tokio::test]
async fn old_schema_migration_keeps_effect_phase_unknown() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let text = scratch.file().to_str().ok_or(HostError::Path)?.to_owned();
    let database = turso::Builder::new_local(text).build().await?;
    let connection = database.connect()?;
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
    assert!(matches!(
        journal
            .advance_launch_phase(row.id, LaunchPhase::JitRequested)
            .await,
        Err(HostError::Journal)
    ));
    assert_eq!(journal.rows().await?.remove(0).launch_phase, None);
    Ok(())
}

#[tokio::test]
async fn invalid_identity_does_not_create_a_row() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = open(&scratch.file()).await?;
    assert_eq!(
        journal
            .begin_prepared_launch("m77r9", &identity("", "w0001"))
            .await,
        Err(HostError::Journal)
    );
    assert!(journal.rows().await?.is_empty());
    Ok(())
}

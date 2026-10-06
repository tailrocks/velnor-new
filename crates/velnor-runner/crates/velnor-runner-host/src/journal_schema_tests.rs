//! Journal schema validation rejects noncanonical identity columns.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Journal;

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-journal-schema-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self { path })
    }

    fn file(&self) -> PathBuf {
        self.path.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.path);
    }
}

struct Snapshot {
    version: i64,
    columns: Vec<String>,
    rows: Vec<(i64, String, String, i64)>,
}

async fn seed(path: &Path, version: i64, table: &str, rows: &str) -> Result<(), String> {
    let text = path
        .to_str()
        .ok_or_else(|| "journal test path is not UTF-8".to_owned())?;
    let db = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    connection
        .execute(table, ())
        .await
        .map_err(|error| error.to_string())?;
    if !rows.is_empty() {
        connection
            .execute(rows, ())
            .await
            .map_err(|error| error.to_string())?;
    }
    connection
        .execute(&format!("PRAGMA user_version = {version}"), ())
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(db);
    Ok(())
}

async fn snapshot(path: &Path) -> Result<Snapshot, String> {
    let text = path
        .to_str()
        .ok_or_else(|| "journal test path is not UTF-8".to_owned())?;
    let db = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    let mut version_query = connection
        .query("PRAGMA user_version", ())
        .await
        .map_err(|error| error.to_string())?;
    let version = version_query
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "user_version row is missing".to_owned())?
        .get::<i64>(0)
        .map_err(|error| error.to_string())?;
    let mut column_query = connection
        .query("PRAGMA table_info(intents)", ())
        .await
        .map_err(|error| error.to_string())?;
    let mut columns = Vec::new();
    while let Some(row) = column_query
        .next()
        .await
        .map_err(|error| error.to_string())?
    {
        columns.push(row.get::<String>(1).map_err(|error| error.to_string())?);
    }
    let mut row_query = connection
        .query(
            "SELECT id, subject, state, cleanup_proven FROM intents ORDER BY subject",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    let mut rows = Vec::new();
    while let Some(row) = row_query.next().await.map_err(|error| error.to_string())? {
        rows.push((
            row.get::<i64>(0).map_err(|error| error.to_string())?,
            row.get::<String>(1).map_err(|error| error.to_string())?,
            row.get::<String>(2).map_err(|error| error.to_string())?,
            row.get::<i64>(3).map_err(|error| error.to_string())?,
        ));
    }
    drop(connection);
    drop(db);
    Ok(Snapshot {
        version,
        columns,
        rows,
    })
}

async fn replace_with_duplicate_ids(path: &Path) -> Result<(), String> {
    let text = path
        .to_str()
        .ok_or_else(|| "journal test path is not UTF-8".to_owned())?;
    let db = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    connection
        .execute("DROP TABLE intents", ())
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT, scale_set_id INTEGER, runner_request_id INTEGER, runner_name TEXT, acquire_attempted INTEGER NOT NULL DEFAULT 0, acquire_resolved INTEGER NOT NULL DEFAULT 0, acquired INTEGER NOT NULL DEFAULT 0, jit_requested INTEGER NOT NULL DEFAULT 0, docker_engine_id TEXT, launch_phase TEXT, launch_id TEXT, assignment_key TEXT, seed_generation_id TEXT, runner_completed INTEGER NOT NULL DEFAULT 0, worker_cleanup_proven INTEGER NOT NULL DEFAULT 0)",
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
    drop(connection);
    drop(db);
    Ok(())
}

#[tokio::test]
async fn current_schema_rejects_duplicate_ids_without_mutating_rows() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    seed(
        &path,
        1,
        "CREATE TABLE intents (id INTEGER, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT, scale_set_id INTEGER, runner_request_id INTEGER, runner_name TEXT, acquire_attempted INTEGER NOT NULL DEFAULT 0, acquire_resolved INTEGER NOT NULL DEFAULT 0, acquired INTEGER NOT NULL DEFAULT 0, jit_requested INTEGER NOT NULL DEFAULT 0, docker_engine_id TEXT, launch_phase TEXT)",
        "INSERT INTO intents (id, kind, subject, state, cleanup_proven) VALUES (7, 'launch', 'first', 'uncertain', 0), (7, 'launch', 'second', 'uncertain', 0)",
    )
    .await?;

    assert!(Journal::open(&path).await.is_err());
    let state = snapshot(&path).await?;
    assert_eq!(state.version, 1);
    assert_eq!(state.columns.len(), 18);
    assert_eq!(state.rows.len(), 2);
    assert!(
        state
            .rows
            .iter()
            .all(|(_, _, status, cleanup)| { status == "uncertain" && *cleanup == 0 })
    );
    Ok(())
}

#[tokio::test]
async fn version_zero_migration_rolls_back_when_id_is_not_primary_key() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    seed(
        &path,
        0,
        "CREATE TABLE intents (id INTEGER, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
        "INSERT INTO intents (id, kind, subject, state, cleanup_proven) VALUES (7, 'launch', 'first', 'pending', 1), (7, 'launch', 'second', 'failed', 1)",
    )
    .await?;

    assert!(Journal::open(&path).await.is_err());
    let state = snapshot(&path).await?;
    assert_eq!(state.version, 0);
    assert_eq!(state.columns.len(), 7);
    assert_eq!(state.rows.len(), 2);
    assert_eq!(state.rows[0].2, "pending");
    assert_eq!(state.rows[0].3, 1);
    assert_eq!(state.rows[1].2, "failed");
    assert_eq!(state.rows[1].3, 1);
    Ok(())
}

#[tokio::test]
async fn current_schema_rejects_wrong_types_and_required_nullability() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    seed(
        &path,
        1,
        "CREATE TABLE intents (id INTEGER PRIMARY KEY, kind BLOB NOT NULL, subject TEXT, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
        "",
    )
    .await?;

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn current_schema_rejects_a_composite_primary_key() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    seed(
        &path,
        1,
        "CREATE TABLE intents (id INTEGER, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT, PRIMARY KEY (id, subject))",
        "",
    )
    .await?;

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn current_schema_rejects_desc_primary_key_without_rowid_alias() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    seed(
        &path,
        1,
        "CREATE TABLE intents (id INTEGER PRIMARY KEY DESC AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
        "",
    )
    .await?;

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn current_schema_rejects_non_autoincrement_id() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    seed(
        &path,
        1,
        "CREATE TABLE intents (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
        "",
    )
    .await?;

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn row_id_updates_rollback_when_identity_becomes_ambiguous() -> Result<(), String> {
    #[derive(Clone, Copy)]
    enum Mutation {
        Finish,
        Bind,
        BindWorker,
        RecordCleanup,
        BindWorkerVolume,
    }

    for mutation in [
        Mutation::Finish,
        Mutation::Bind,
        Mutation::BindWorker,
        Mutation::RecordCleanup,
        Mutation::BindWorkerVolume,
    ] {
        let scratch = Scratch::new()?;
        let path = scratch.file();
        let journal = Journal::open(&path)
            .await
            .map_err(|error| error.to_string())?;
        replace_with_duplicate_ids(&path).await?;
        let result = match mutation {
            Mutation::Finish => journal.finish(7, crate::Outcome::DefiniteFailure).await,
            Mutation::Bind => journal.bind(7, Some("runner"), Some("github")).await,
            Mutation::BindWorker => journal.bind_worker(7, Some("runner"), Some("dind")).await,
            Mutation::RecordCleanup => journal.record_cleanup(7).await,
            Mutation::BindWorkerVolume => journal.bind_worker_volume(7, "volume").await,
        };
        assert_eq!(result, Err(crate::HostError::Journal));
        let state = snapshot(&path).await?;
        assert_eq!(state.rows.len(), 2);
        assert!(
            state
                .rows
                .iter()
                .all(|row| { row.2 == "uncertain" && row.3 == 0 })
        );
        assert!(
            journal
                .rows()
                .await
                .map_err(|error| error.to_string())?
                .iter()
                .all(|row| {
                    row.docker_id.is_none()
                        && row.github_runner_id.is_none()
                        && row.dind_id.is_none()
                        && row.worker_volume.is_none()
                })
        );
    }
    Ok(())
}

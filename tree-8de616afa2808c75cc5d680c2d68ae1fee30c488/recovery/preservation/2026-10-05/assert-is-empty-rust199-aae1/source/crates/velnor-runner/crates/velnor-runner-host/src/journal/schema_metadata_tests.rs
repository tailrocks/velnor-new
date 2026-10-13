//! Regressions for fail-closed journal schema admission.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::Journal;

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-journal-schema-{label}-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self { path })
    }

    fn file(&self) -> PathBuf {
        self.path.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _cleanup = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ColumnSnapshot {
    cid: i64,
    name: String,
    declared_type: String,
    not_null: i64,
    default_value: Option<String>,
    primary_key: i64,
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    version: i64,
    create_sql: String,
    columns: Vec<ColumnSnapshot>,
    rows: Vec<Vec<String>>,
}

fn create_schema(id: &str, state: &str, cleanup: &str) -> String {
    format!(
        "CREATE TABLE intents (id {id}, kind TEXT NOT NULL, subject TEXT NOT NULL, state {state}, docker_id TEXT, github_runner_id TEXT, cleanup_proven {cleanup}, dind_id TEXT, worker_volume TEXT)"
    )
}

async fn database(path: &Path) -> Result<(turso::Database, turso::Connection), String> {
    let path = path
        .to_str()
        .ok_or_else(|| "journal path is not UTF-8".to_owned())?;
    let db = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    Ok((db, connection))
}

async fn snapshot(path: &Path) -> Result<Snapshot, String> {
    let (db, connection) = database(path).await?;
    let version = {
        let mut rows = connection
            .query("PRAGMA user_version", ())
            .await
            .map_err(|error| error.to_string())?;
        rows.next()
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "user_version returned no row".to_owned())?
            .get::<i64>(0)
            .map_err(|error| error.to_string())?
    };
    let create_sql = {
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'intents'",
                (),
            )
            .await
            .map_err(|error| error.to_string())?;
        rows.next()
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "intents table missing".to_owned())?
            .get::<String>(0)
            .map_err(|error| error.to_string())?
    };
    let columns = read_columns(&connection).await?;
    let rows = read_rows(&connection).await?;
    drop(connection);
    drop(db);
    Ok(Snapshot {
        version,
        create_sql,
        columns,
        rows,
    })
}

async fn read_columns(connection: &turso::Connection) -> Result<Vec<ColumnSnapshot>, String> {
    let mut cursor = connection
        .query("PRAGMA table_info(intents)", ())
        .await
        .map_err(|error| error.to_string())?;
    let mut columns = Vec::new();
    while let Some(row) = cursor.next().await.map_err(|error| error.to_string())? {
        columns.push(ColumnSnapshot {
            cid: row.get::<i64>(0).map_err(|error| error.to_string())?,
            name: row.get::<String>(1).map_err(|error| error.to_string())?,
            declared_type: row.get::<String>(2).map_err(|error| error.to_string())?,
            not_null: row.get::<i64>(3).map_err(|error| error.to_string())?,
            default_value: row
                .get::<Option<String>>(4)
                .map_err(|error| error.to_string())?,
            primary_key: row.get::<i64>(5).map_err(|error| error.to_string())?,
        });
    }
    Ok(columns)
}

async fn read_rows(connection: &turso::Connection) -> Result<Vec<Vec<String>>, String> {
    let mut cursor = connection
        .query(
            "SELECT quote(id), quote(kind), quote(subject), quote(state), quote(docker_id), quote(github_runner_id), quote(cleanup_proven), quote(dind_id), quote(worker_volume) FROM intents ORDER BY rowid",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    let mut rows = Vec::new();
    while let Some(row) = cursor.next().await.map_err(|error| error.to_string())? {
        let mut values = Vec::new();
        for index in 0..9 {
            values.push(
                row.get::<String>(index)
                    .map_err(|error| error.to_string())?,
            );
        }
        rows.push(values);
    }
    Ok(rows)
}

async fn reject_without_mutation(
    label: &str,
    id: &str,
    state: &str,
    cleanup: &str,
    version: i64,
) -> Result<(), String> {
    let scratch = Scratch::new(label)?;
    let path = scratch.file();
    let (db, connection) = database(&path).await?;
    connection
        .execute(&create_schema(id, state, cleanup), ())
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO intents (id, kind, subject, state, docker_id, github_runner_id, cleanup_proven, dind_id, worker_volume) VALUES (17, 'launch', 'legacy', 'failed', 'docker-1', 'runner-1', 1, 'dind-1', 'volume-1')",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(&format!("PRAGMA user_version = {version}"), ())
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(db);

    let before = snapshot(&path).await?;
    assert!(Journal::open(&path).await.is_err());
    assert_eq!(snapshot(&path).await?, before);
    Ok(())
}

#[tokio::test]
async fn malformed_version_zero_metadata_is_rejected_without_mutation() -> Result<(), String> {
    let cases = [
        (
            "missing-id-primary-key",
            "INTEGER",
            "TEXT NOT NULL",
            "INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "descending-id-primary-key-is-not-rowid-alias",
            "INTEGER PRIMARY KEY DESC",
            "TEXT NOT NULL",
            "INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "wrong-id-type",
            "TEXT PRIMARY KEY",
            "TEXT NOT NULL",
            "INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "wrong-state-type",
            "INTEGER PRIMARY KEY AUTOINCREMENT",
            "BLOB NOT NULL",
            "INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "nullable-state",
            "INTEGER PRIMARY KEY AUTOINCREMENT",
            "TEXT",
            "INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "wrong-cleanup-default",
            "INTEGER PRIMARY KEY AUTOINCREMENT",
            "TEXT NOT NULL",
            "INTEGER NOT NULL DEFAULT 1",
        ),
        (
            "missing-cleanup-default",
            "INTEGER PRIMARY KEY AUTOINCREMENT",
            "TEXT NOT NULL",
            "INTEGER NOT NULL",
        ),
    ];
    for (label, id, state, cleanup) in cases {
        reject_without_mutation(label, id, state, cleanup, 0).await?;
    }
    Ok(())
}

#[tokio::test]
async fn malformed_current_schema_is_rejected_without_mutation() -> Result<(), String> {
    reject_without_mutation(
        "current-cleanup-default",
        "INTEGER PRIMARY KEY AUTOINCREMENT",
        "TEXT NOT NULL",
        "INTEGER NOT NULL DEFAULT 1",
        1,
    )
    .await
}

#[tokio::test]
async fn reopened_current_schema_keeps_distinct_insert_ids() -> Result<(), String> {
    let scratch = Scratch::new("valid-reopen")?;
    let path = scratch.file();
    let first = {
        let journal = Journal::open(&path)
            .await
            .map_err(|error| error.to_string())?;
        journal
            .begin("launch", "first")
            .await
            .map_err(|error| error.to_string())?
    };
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let second = journal
        .begin("launch", "second")
        .await
        .map_err(|error| error.to_string())?;
    assert_ne!(first, second);
    assert_eq!(journal.read(first).await, Ok(super::IntentState::Pending));
    assert_eq!(journal.read(second).await, Ok(super::IntentState::Pending));
    Ok(())
}

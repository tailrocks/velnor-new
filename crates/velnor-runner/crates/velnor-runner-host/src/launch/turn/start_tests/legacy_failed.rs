//! Older journals must not free ambiguous launches during restart.

use std::path::Path;

use crate::launch::inspect_tests::DockerStub;
use crate::launch_harness::{Mode, Scratch, Script, assigned_wait};
use crate::{IntentState, Journal, Outcome};

async fn seed_legacy_journal(path: &Path) -> Result<(), String> {
    let path = path
        .to_str()
        .ok_or_else(|| "legacy journal path is not UTF-8".to_owned())?;
    let db = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO intents (kind, subject, state, cleanup_proven) VALUES ('launch', 'm96', 'failed', 1), ('launch', 'm98', 'uncertain', 1), ('launch', 'm99', 'pending', 1), ('launch', 'm100', 'done', 1), ('acquire', 'old-acquire', 'failed', 1)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(db);
    Ok(())
}

async fn assert_assignment_is_held(journal: &Journal, message_id: i64) -> Result<(), String> {
    let session = super::zero_assignment_session()?;
    let polled = assigned_wait(message_id, 1);
    let docker = DockerStub::open(Vec::new())?;
    let decision = crate::launch::admission(&docker.docker, journal, 1, 1, 0, &polled)
        .await
        .map_err(|error| error.to_string())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::JitConflict,
    };
    let mut workers = Vec::new();
    let result = super::start_turn(
        &mut script,
        &mut workers,
        super::StartTurn {
            ready: super::ready(&session, &polled),
            journal,
            docker: &docker.docker,
            capacity: 1,
            rest: super::rest(),
            stop: false,
        },
    )
    .await;
    docker.finish().await?;

    assert_eq!(decision, crate::launch::Admit::Hold);
    assert_eq!(result, Ok(false));
    assert_eq!(script.calls, [] as [&str; 0]);
    assert_eq!(workers, [] as [worker::projection_types::Started; 0]);
    Ok(())
}

async fn read_user_version(connection: &turso::Connection) -> Result<i64, String> {
    let mut rows = connection
        .query("PRAGMA user_version", ())
        .await
        .map_err(|error| error.to_string())?;
    rows.next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "user_version did not return a row".to_owned())?
        .get::<i64>(0)
        .map_err(|error| error.to_string())
}

#[tokio::test]
async fn legacy_ambiguous_launch_rows_are_quarantined_before_restart() -> Result<(), String> {
    let scratch = Scratch::new("legacy-failed-launch").map_err(|error| error.to_string())?;
    let path = scratch.file();
    seed_legacy_journal(&path).await?;
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    for (subject, expected) in [
        ("m96", IntentState::Uncertain),
        ("m98", IntentState::Uncertain),
        ("m99", IntentState::Pending),
    ] {
        let row = rows
            .iter()
            .find(|row| row.subject == subject)
            .ok_or_else(|| format!("legacy launch {subject} disappeared"))?;
        assert_eq!(row.state, expected);
        assert!(!row.cleanup_proven);
    }
    let completed = rows
        .iter()
        .find(|row| row.subject == "m100")
        .ok_or_else(|| "completed row disappeared".to_owned())?;
    assert_eq!(completed.state, IntentState::Done);
    assert!(completed.cleanup_proven);
    let unrelated = rows
        .iter()
        .find(|row| row.subject == "old-acquire")
        .ok_or_else(|| "unrelated row disappeared".to_owned())?;
    assert_eq!(unrelated.state, IntentState::Failed);
    assert!(unrelated.cleanup_proven);

    assert_eq!(crate::launch::slot::occupied(&journal).await, Ok(3));
    for message_id in [96, 98, 99] {
        assert_assignment_is_held(&journal, message_id).await?;
    }
    Ok(())
}

#[tokio::test]
async fn new_definite_failures_remain_retryable_after_migration() -> Result<(), String> {
    let scratch = Scratch::new("new-definite-failure").map_err(|error| error.to_string())?;
    let path = scratch.file();
    seed_legacy_journal(&path).await?;
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let fresh = journal
        .begin_launch("m97")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh.1);
    journal
        .finish(fresh.0, Outcome::DefiniteFailure)
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);
    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let persisted = reopened.rows().await.map_err(|error| error.to_string())?;
    let definite = persisted
        .iter()
        .find(|row| row.subject == "m97")
        .ok_or_else(|| "new definite failure disappeared".to_owned())?;
    assert_eq!(definite.state, IntentState::Failed);
    let retry = reopened
        .begin_launch("m97")
        .await
        .map_err(|error| error.to_string())?;
    assert!(retry.1);
    assert_ne!(retry.0, fresh.0);
    Ok(())
}

#[tokio::test]
async fn future_journal_version_is_rejected_without_creating_tables() -> Result<(), String> {
    let scratch = Scratch::new("future-journal-version").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let path_string = path
        .to_str()
        .ok_or_else(|| "future journal path is not UTF-8".to_owned())?;
    let db = turso::Builder::new_local(path_string)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    connection
        .execute("PRAGMA user_version = 2", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(db);

    assert!(Journal::open(&path).await.is_err());
    let db = turso::Builder::new_local(path_string)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    let mut rows = connection
        .query("SELECT name FROM sqlite_master WHERE type = 'table'", ())
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        rows.next()
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    assert_eq!(read_user_version(&connection).await?, 2);
    Ok(())
}

#[tokio::test]
async fn failed_legacy_migration_rolls_back_schema_and_rows() -> Result<(), String> {
    let scratch = Scratch::new("legacy-migration-rollback").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let path_string = path
        .to_str()
        .ok_or_else(|| "malformed journal path is not UTF-8".to_owned())?;
    let db = turso::Builder::new_local(path_string)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO intents (kind, subject, state) VALUES ('launch', 'm101', 'failed')",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(db);

    assert!(Journal::open(&path).await.is_err());
    let db = turso::Builder::new_local(path_string)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    let mut columns = connection
        .query("PRAGMA table_info(intents)", ())
        .await
        .map_err(|error| error.to_string())?;
    let mut names = Vec::new();
    while let Some(row) = columns.next().await.map_err(|error| error.to_string())? {
        names.push(row.get::<String>(1).map_err(|error| error.to_string())?);
    }
    assert!(!names.iter().any(|name| name == "dind_id"));
    assert!(!names.iter().any(|name| name == "worker_volume"));
    assert_eq!(read_user_version(&connection).await?, 0);
    let mut rows = connection
        .query("SELECT state FROM intents WHERE subject = 'm101'", ())
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "malformed journal row disappeared".to_owned())?;
    assert_eq!(
        row.get::<String>(0).map_err(|error| error.to_string())?,
        "failed"
    );
    Ok(())
}

#[tokio::test]
async fn malformed_unused_column_rolls_back_before_row_migration() -> Result<(), String> {
    let scratch =
        Scratch::new("legacy-missing-subject-column").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let path_string = path
        .to_str()
        .ok_or_else(|| "malformed journal path is not UTF-8".to_owned())?;
    let db = turso::Builder::new_local(path_string)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO intents (kind, state, cleanup_proven) VALUES ('launch', 'failed', 1)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(db);

    assert!(Journal::open(&path).await.is_err());
    let db = turso::Builder::new_local(path_string)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = db.connect().map_err(|error| error.to_string())?;
    let mut columns = connection
        .query("PRAGMA table_info(intents)", ())
        .await
        .map_err(|error| error.to_string())?;
    let mut names = Vec::new();
    while let Some(row) = columns.next().await.map_err(|error| error.to_string())? {
        names.push(row.get::<String>(1).map_err(|error| error.to_string())?);
    }
    assert!(!names.iter().any(|name| name == "dind_id"));
    assert!(!names.iter().any(|name| name == "worker_volume"));
    assert!(!names.iter().any(|name| name == "subject"));
    assert_eq!(read_user_version(&connection).await?, 0);
    let mut rows = connection
        .query(
            "SELECT state, cleanup_proven FROM intents WHERE kind = 'launch'",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "malformed journal row disappeared".to_owned())?;
    assert_eq!(
        row.get::<String>(0).map_err(|error| error.to_string())?,
        "failed"
    );
    assert_eq!(row.get::<i64>(1).map_err(|error| error.to_string())?, 1);
    Ok(())
}

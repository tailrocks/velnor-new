// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::journal::Journal;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const TABLE: &str = "connection_scale_set_deletes";

async fn execute(path: &Path, statements: &[&str]) -> TestResult {
    let journal = Journal::open(path).await?;
    let connection = journal.connection().await?;
    for statement in statements {
        connection.execute(*statement, ()).await?;
    }
    Ok(())
}

async fn scalar(path: &Path, query: &str) -> TestResult<i64> {
    let journal = Journal::open(path).await?;
    let connection = journal.connection().await?;
    let mut rows = connection.query(query, ()).await?;
    Ok(rows.next().await?.ok_or("missing row")?.get::<i64>(0)?)
}

async fn journal_below_the_table(version: i64) -> TestResult<tempfile::TempDir> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.db");
    execute(
        &path,
        &[
            &format!("DROP TABLE {TABLE}"),
            &format!("PRAGMA user_version = {version}"),
        ],
    )
    .await?;
    Ok(directory)
}

#[tokio::test]
async fn journals_below_the_table_upgrade_to_v58_and_gain_it() -> TestResult {
    for version in [57_i64, 52] {
        let directory = journal_below_the_table(version).await?;
        let path = directory.path().join("journal.db");
        assert_eq!(scalar(&path, "PRAGMA user_version").await?, 58);
        let tables =
            format!("SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = '{TABLE}'");
        assert_eq!(scalar(&path, &tables).await?, 1);
        let rows = format!("SELECT COUNT(*) FROM {TABLE}");
        assert_eq!(scalar(&path, &rows).await?, 0);
    }
    Ok(())
}

#[tokio::test]
async fn a_v58_journal_missing_the_table_is_rejected() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.db");
    execute(&path, &[&format!("DROP TABLE {TABLE}")]).await?;
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn a_v58_journal_with_a_widened_table_is_rejected() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.db");
    execute(
        &path,
        &[
            &format!("DROP TABLE {TABLE}"),
            "CREATE TABLE connection_scale_set_deletes (id INTEGER PRIMARY KEY AUTOINCREMENT, connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), runner_group_id INTEGER NOT NULL CHECK (runner_group_id > 0), state TEXT NOT NULL CHECK (state IN ('prepared', 'requested', 'uncertain', 'deleted', 'anything')), UNIQUE (connection_id))",
        ],
    )
    .await?;
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn versions_outside_the_migration_chain_stay_fail_closed() -> TestResult {
    for version in [53_i64, 56, 59] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("journal.db");
        execute(&path, &[&format!("PRAGMA user_version = {version}")]).await?;
        assert!(Journal::open(&path).await.is_err(), "version {version}");
    }
    Ok(())
}

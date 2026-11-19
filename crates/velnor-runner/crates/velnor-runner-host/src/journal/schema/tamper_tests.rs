// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const GUARD: &str = "storage_initialization_insert_guard";

async fn trigger_sql(journal: &Journal, name: &str) -> TestResult<String> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
            [name.to_owned()],
        )
        .await?;
    Ok(rows.next().await?.ok_or("missing trigger")?.get(0)?)
}

async fn execute(journal: &Journal, sql: &str) -> TestResult<()> {
    journal.connection().await?.execute(sql, ()).await?;
    Ok(())
}

async fn replace_guard(journal: &Journal, rewrite: impl Fn(&str) -> String) -> TestResult<()> {
    let original = trigger_sql(journal, GUARD).await?;
    execute(journal, &format!("DROP TRIGGER {GUARD}")).await?;
    execute(journal, &rewrite(&original)).await?;
    Ok(())
}

#[tokio::test]
async fn a_current_version_journal_with_a_weakened_trigger_is_rejected() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.db");
    let journal = Journal::open(&path).await?;
    replace_guard(&journal, |sql| {
        sql.replace("intent.kind = 'launch'", "intent.kind = 'x'")
    })
    .await?;
    drop(journal);
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn a_current_version_journal_with_an_extra_column_is_rejected() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.db");
    let journal = Journal::open(&path).await?;
    execute(
        &journal,
        "ALTER TABLE capacity_reservation_storage_initialization ADD COLUMN tampered TEXT",
    )
    .await?;
    drop(journal);
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn a_current_version_journal_with_a_reformatted_trigger_is_accepted() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.db");
    let journal = Journal::open(&path).await?;
    replace_guard(&journal, |sql| {
        sql.replace(" WHEN ", "\n    when  ")
            .replace(" BEGIN ", "\n begin\n")
    })
    .await?;
    drop(journal);
    Journal::open(&path).await?;
    Ok(())
}

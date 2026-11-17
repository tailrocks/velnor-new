// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn a_historical_journal_upgrades_and_rejects_a_replaced_recovery_source() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("historical.db");
    let historical = Journal::open_historical_v45_for_test(&path).await?;
    assert_eq!(version(&historical).await?, 45);
    let receipt_table = text(
        &historical,
        "SELECT sql FROM sqlite_schema WHERE type='table' AND name='physical_attempt_jit_conflict_receipts'",
    )
    .await?;
    drop(historical);

    let upgraded = Journal::open(&path).await?;
    assert_eq!(version(&upgraded).await?, 46);
    assert_eq!(
        text(
            &upgraded,
            "SELECT sql FROM sqlite_schema WHERE type='table' AND name='physical_attempt_jit_conflict_receipts'",
        )
        .await?,
        receipt_table
    );
    drop(upgraded);
    let upgraded = Journal::open(&path).await?;
    let connection = upgraded.connection().await?;
    connection
        .execute("DROP VIEW physical_attempt_jit_recovery_sources", ())
        .await?;
    connection
        .execute(
            &format!(
                "CREATE VIEW physical_attempt_jit_recovery_sources AS SELECT {}, 'conflict' AS source_kind FROM physical_attempt_jit_conflict_receipts",
                super::ddl::COLUMNS
            ),
            (),
        )
        .await?;
    drop(connection);
    drop(upgraded);
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

async fn version(journal: &Journal) -> TestResult<i64> {
    let connection = journal.connection().await?;
    let mut rows = connection.query("PRAGMA user_version", ()).await?;
    Ok(rows
        .next()
        .await?
        .ok_or("missing journal version")?
        .get(0)?)
}

async fn text(journal: &Journal, sql: &str) -> TestResult<String> {
    let connection = journal.connection().await?;
    let mut rows = connection.query(sql, ()).await?;
    Ok(rows.next().await?.ok_or("missing schema object")?.get(0)?)
}

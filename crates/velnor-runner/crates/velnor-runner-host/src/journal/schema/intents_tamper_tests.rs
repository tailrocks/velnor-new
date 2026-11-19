// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn an_extra_intents_column_is_rejected_at_the_current_version() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.db");
    let journal = Journal::open(&path).await?;
    let connection = journal.connection().await?;
    connection
        .execute("ALTER TABLE intents ADD COLUMN tampered TEXT", ())
        .await?;
    drop(connection);
    drop(journal);
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

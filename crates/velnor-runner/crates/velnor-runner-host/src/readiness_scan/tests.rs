// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use crate::error::HostError;

use super::{JournalFact, MAX_JOURNAL_ROWS, journal_mark};

async fn journal_with(path: &Path, occupying_first: bool, rows: usize) -> Result<(), HostError> {
    let db = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let conn = db.connect().map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    let values: Vec<String> = (0..rows)
        .map(|index| {
            let proven = i32::from(!(occupying_first && index == 0));
            format!("('launch', 's{index}', 'pending', {proven})")
        })
        .collect();
    conn.execute(
        &format!(
            "INSERT INTO intents (kind, subject, state, cleanup_proven) VALUES {}",
            values.join(",")
        ),
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn mark(rows: usize) -> Result<JournalFact, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("journal.db");
    journal_with(&path, true, rows).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    Ok(journal_mark(&path, deadline).await)
}

#[tokio::test]
async fn an_occupying_first_row_does_not_hide_an_over_cap_journal()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(mark(MAX_JOURNAL_ROWS + 1).await?, JournalFact::Unreadable);
    Ok(())
}

#[tokio::test]
async fn one_occupying_row_alone_is_occupied_not_unreadable()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(mark(1).await?, JournalFact::Occupied);
    Ok(())
}

#[tokio::test]
async fn an_occupying_first_row_at_exactly_the_cap_is_occupied()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(mark(MAX_JOURNAL_ROWS).await?, JournalFact::Occupied);
    Ok(())
}

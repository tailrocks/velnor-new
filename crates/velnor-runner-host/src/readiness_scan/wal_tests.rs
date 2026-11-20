// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{JournalFact, MAX_JOURNAL_BYTES, journal_mark, journal_mark_with};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Journal {
    _dir: tempfile::TempDir,
    path: PathBuf,
    wal: PathBuf,
}

async fn journal_with_wal() -> Result<Journal, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let origin = dir.path().join("origin");
    let live = dir.path().join("live");
    std::fs::create_dir(&origin)?;
    std::fs::create_dir(&live)?;
    let db = turso::Builder::new_local(origin.join("launch.db").to_str().ok_or("path")?)
        .build()
        .await?;
    let conn = db.connect()?;
    conn.execute(
        "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
        (),
    )
    .await?;
    conn.execute(
        "CREATE TABLE connection_intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
        (),
    )
    .await?;
    conn.execute(
        "INSERT INTO intents (kind, subject, state, cleanup_proven) VALUES ('launch', 's', 'done', 1)",
        (),
    )
    .await?;
    let path = live.join("launch.db");
    let wal = live.join("launch.db-wal");
    std::fs::copy(origin.join("launch.db"), &path)?;
    std::fs::copy(origin.join("launch.db-wal"), &wal)?;
    assert!(std::fs::metadata(&wal)?.len() > 0);
    Ok(Journal {
        _dir: dir,
        path,
        wal,
    })
}

fn grow(path: &Path, len: u64) -> TestResult {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .set_len(len)?;
    Ok(())
}

async fn mark(path: &Path) -> JournalFact {
    journal_mark(path, tokio::time::Instant::now() + Duration::from_secs(30)).await
}

#[tokio::test]
async fn a_small_journal_with_a_small_wal_is_clear() -> TestResult {
    let journal = journal_with_wal().await?;
    assert_eq!(mark(&journal.path).await, JournalFact::Clear);
    Ok(())
}

#[tokio::test]
async fn a_wal_that_pushes_the_pair_over_the_cap_is_unreadable() -> TestResult {
    let journal = journal_with_wal().await?;
    let main = std::fs::metadata(&journal.path)?.len();
    grow(&journal.wal, MAX_JOURNAL_BYTES - main + 1)?;
    assert_eq!(mark(&journal.path).await, JournalFact::Unreadable);
    Ok(())
}

#[tokio::test]
async fn a_pair_of_exactly_the_cap_is_not_failed_for_size() -> TestResult {
    let journal = journal_with_wal().await?;
    let main = std::fs::metadata(&journal.path)?.len();
    grow(&journal.wal, MAX_JOURNAL_BYTES - main)?;
    assert_eq!(mark(&journal.path).await, JournalFact::Clear);
    Ok(())
}

#[tokio::test]
async fn a_symlinked_wal_is_unreadable() -> TestResult {
    let journal = journal_with_wal().await?;
    let target = journal.wal.with_file_name("wal-target");
    std::fs::rename(&journal.wal, &target)?;
    std::os::unix::fs::symlink(&target, &journal.wal)?;
    assert_eq!(mark(&journal.path).await, JournalFact::Unreadable);
    Ok(())
}

#[tokio::test]
async fn a_wal_that_grows_past_the_cap_during_the_scan_is_unreadable() -> TestResult {
    let journal = journal_with_wal().await?;
    let wal = journal.wal.clone();
    let scan = async move {
        grow(&wal, MAX_JOURNAL_BYTES + 1).map_err(|_| crate::error::HostError::Journal)?;
        Ok(false)
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    assert_eq!(
        journal_mark_with(&journal.path, deadline, scan).await,
        JournalFact::Unreadable
    );
    Ok(())
}

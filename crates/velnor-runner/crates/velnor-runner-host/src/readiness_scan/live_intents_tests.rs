// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::journal::Journal;

use super::{JournalFact, MAX_JOURNAL_ROWS, journal_mark};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn connection() -> Result<RepositoryConnection, Box<dyn std::error::Error>> {
    Ok(RepositoryConnection::try_new(
        ConnectionId::parse("conn-test-1")?,
        42,
        "owner/repo".to_owned(),
        KeychainReference::parse("keychain:velnor.test/conn-test-1")?,
        ScaleSetSelector::try_new(
            "ubuntu-26.04-scale-set",
            &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
        )?,
        SourcePolicy::try_new(
            SourcePolicyMode::BranchRestricted,
            vec!["main".to_owned()],
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )?,
    )?)
}

async fn mark_with(
    live: usize,
    kind: &str,
    legacy: usize,
) -> Result<JournalFact, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("launch.db");
    seed(&path, live, kind, legacy).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    Ok(journal_mark(&path, deadline).await)
}

async fn seed(path: &Path, live: usize, kind: &str, legacy: usize) -> TestResult {
    let journal = Journal::open(path).await?;
    journal.connection_scope(&connection()?).await?;
    let conn = journal.connection().await?;
    let live_rows: Vec<String> = (0..live)
        .map(|index| format!("('conn-test-1', '{kind}', 's{index}', 'pending', 0)"))
        .collect();
    if !live_rows.is_empty() {
        conn.execute(
            &format!(
                "INSERT INTO connection_intents (connection_id, kind, subject, state, cleanup_proven) VALUES {}",
                live_rows.join(",")
            ),
            (),
        )
        .await?;
    }
    let legacy_rows: Vec<String> = (0..legacy)
        .map(|index| format!("('launch', 'l{index}', 'pending', 0)"))
        .collect();
    if !legacy_rows.is_empty() {
        conn.execute(
            &format!(
                "INSERT INTO intents (kind, subject, state, cleanup_proven) VALUES {}",
                legacy_rows.join(",")
            ),
            (),
        )
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn an_empty_journal_is_clear() -> TestResult {
    assert_eq!(mark_with(0, "launch", 0).await?, JournalFact::Clear);
    Ok(())
}

#[tokio::test]
async fn live_launches_past_the_row_cap_are_unreadable() -> TestResult {
    assert_eq!(
        mark_with(MAX_JOURNAL_ROWS + 1, "launch", 0).await?,
        JournalFact::Unreadable
    );
    Ok(())
}

#[tokio::test]
async fn live_launches_at_the_row_cap_are_occupied() -> TestResult {
    assert_eq!(
        mark_with(MAX_JOURNAL_ROWS, "launch", 0).await?,
        JournalFact::Occupied
    );
    Ok(())
}

#[tokio::test]
async fn one_live_launch_is_occupied_not_unreadable() -> TestResult {
    assert_eq!(mark_with(1, "launch", 0).await?, JournalFact::Occupied);
    Ok(())
}

#[tokio::test]
async fn a_live_row_of_unknown_kind_is_unreadable() -> TestResult {
    assert_eq!(mark_with(1, "bogus", 0).await?, JournalFact::Unreadable);
    Ok(())
}

#[tokio::test]
async fn legacy_intents_still_count() -> TestResult {
    assert_eq!(mark_with(0, "launch", 1).await?, JournalFact::Occupied);
    Ok(())
}

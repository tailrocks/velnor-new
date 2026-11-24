// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{TestResult, advance, scenario};
use crate::journal::Journal;

async fn refuses_bootstrap_absent(
    runner: (&str, Option<&str>),
    dind: (&str, Option<&str>),
    corrupted: &[&str],
) -> TestResult<()> {
    let (_state, journal) = scenario(runner, dind, corrupted).await?;
    let error = advance(&journal, "claimed", "bootstrap_absent")
        .await?
        .expect_err("bootstrap_absent accepted with the other bootstrap container past pre-start");
    assert!(
        error
            .to_string()
            .contains("invalid pre-JIT cleanup progress update"),
        "refusal came from the progress guard: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn bootstrap_absent_is_refused_when_the_other_container_create_is_requested() -> TestResult<()>
{
    refuses_bootstrap_absent(("uncertain", None), ("requested", None), &[]).await
}

#[tokio::test]
async fn bootstrap_absent_is_refused_when_the_other_container_is_started() -> TestResult<()> {
    refuses_bootstrap_absent(
        ("uncertain", None),
        ("created", Some("running-dind")),
        &["UPDATE capacity_reservation_bootstrap_containers SET start_state = 'started' WHERE role = 'dind'"],
    )
    .await
}

#[tokio::test]
async fn bootstrap_absent_is_refused_when_the_other_container_start_is_requested() -> TestResult<()>
{
    refuses_bootstrap_absent(
        ("uncertain", None),
        ("created", Some("starting-dind")),
        &["UPDATE capacity_reservation_bootstrap_containers SET start_state = 'requested' WHERE role = 'dind'"],
    )
    .await
}

#[tokio::test]
async fn bootstrap_absent_is_accepted_when_the_other_container_was_never_claimed() -> TestResult<()>
{
    let (_state, journal) = scenario(("uncertain", None), ("unclaimed", None), &[]).await?;
    advance(&journal, "claimed", "bootstrap_absent")
        .await?
        .expect("an unclaimed other container keeps the claim pre-start");
    Ok(())
}

#[tokio::test]
async fn a_version_54_guard_journal_migrates_to_the_current_guard_on_open() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("launch.db");
    {
        let journal = Journal::open(&path).await?;
        let connection = journal.connection().await?;
        connection.execute("PRAGMA user_version = 59", ()).await?;
        connection
            .execute("DROP TRIGGER pre_jit_cleanup_progress_update_guard", ())
            .await?;
        connection
            .execute(super::super::PROGRESS_UPDATE_GUARD_V54, ())
            .await?;
    }
    let migrated = Journal::open(&path).await?;
    let connection = migrated.connection().await?;
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'pre_jit_cleanup_progress_update_guard'",
            (),
        )
        .await?;
    let sql: String = rows
        .next()
        .await?
        .ok_or("trigger missing after migration")?
        .get(0)?;
    assert_eq!(
        super::super::super::sql_normalization::normalize(&sql),
        super::super::super::sql_normalization::normalize(super::super::PROGRESS_UPDATE_GUARD)
    );
    Ok(())
}

#[tokio::test]
async fn a_current_journal_with_the_version_54_guard_is_refused_at_open() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("launch.db");
    {
        let journal = Journal::open(&path).await?;
        let connection = journal.connection().await?;
        connection
            .execute("DROP TRIGGER pre_jit_cleanup_progress_update_guard", ())
            .await?;
        connection
            .execute(super::super::PROGRESS_UPDATE_GUARD_V54, ())
            .await?;
    }
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

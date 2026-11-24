// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

async fn stored_guards(connection: &turso::Connection) -> TestResult<Vec<(String, String)>> {
    let mut stored = Vec::new();
    for (name, _) in super::guards(connection).await? {
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
                [name.as_str()],
            )
            .await?;
        let sql: String = rows.next().await?.ok_or("guard missing")?.get(0)?;
        stored.push((name, sql));
    }
    Ok(stored)
}

#[tokio::test]
async fn a_journal_with_exact_clock_guards_migrates_to_one_tick_windows_on_open() -> TestResult<()>
{
    let state = tempfile::tempdir()?;
    let path = state.path().join("launch.db");
    let fresh = {
        let journal = Journal::open(&path).await?;
        let connection = journal.connection().await?;
        let fresh = stored_guards(&connection).await?;
        super::restore_exact_guards_at_v60(&connection).await?;
        let exact = stored_guards(&connection).await?;
        assert!(
            fresh
                .iter()
                .zip(&exact)
                .all(|(window, exact)| window != exact),
            "every guard must start from its exact form"
        );
        fresh
    };
    let migrated = Journal::open(&path).await?;
    let connection = migrated.connection().await?;
    let mut rows = connection.query("PRAGMA user_version", ()).await?;
    let version: i64 = rows.next().await?.ok_or("version missing")?.get(0)?;
    assert_eq!(version, super::VERSION);
    assert_eq!(stored_guards(&connection).await?, fresh);
    Ok(())
}

#[tokio::test]
async fn a_current_journal_with_any_one_exact_clock_guard_is_refused_at_open() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let guards = {
        let journal = Journal::open(&state.path().join("guards.db")).await?;
        super::guards(&journal.connection().await?).await?
    };
    assert_eq!(guards.len(), 9);
    for (index, (name, sql)) in guards.iter().enumerate() {
        let path = state.path().join(format!("guard-{index}.db"));
        {
            let journal = Journal::open(&path).await?;
            let connection = journal.connection().await?;
            super::recreate(&connection, name, &super::exact(sql)).await?;
        }
        assert!(
            Journal::open(&path).await.is_err(),
            "a v{} journal with only {name} in its exact form must be refused",
            super::VERSION
        );
    }
    Ok(())
}

#[tokio::test]
async fn an_altered_version_60_clock_guard_is_refused_before_it_is_replaced() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("launch.db");
    {
        let journal = Journal::open(&path).await?;
        let connection = journal.connection().await?;
        super::restore_exact_guards_at_v60(&connection).await?;
        connection
            .execute("DROP TRIGGER bootstrap_claim_clock_anchor_guard", ())
            .await?;
        connection
            .execute(
                "CREATE TRIGGER bootstrap_claim_clock_anchor_guard BEFORE UPDATE ON bootstrap_create_claim_clocks BEGIN SELECT 1; END",
                (),
            )
            .await?;
    }
    assert!(
        Journal::open(&path).await.is_err(),
        "a tampered v60 guard must be refused, not silently replaced by the v61 guard"
    );
    Ok(())
}

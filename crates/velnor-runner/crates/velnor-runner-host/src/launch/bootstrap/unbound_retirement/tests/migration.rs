// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::{IntentState, Journal, Outcome};

use super::TestResult;

const TABLES: [&str; 1] = ["intents"];

const GUARDS: [&str; 3] = [
    "unbound_retirement_claim_insert_guard",
    "unbound_retirement_progress_insert_guard",
    "physical_attempt_unbound_retirement_insert_guard",
];

const V44_SHAPE_START: &str = "state.resources_bound = 0 AND";
const V44_SHAPE_END: &str = "init.abort_state = 'absent')))";
const V43_SHAPE: &str = "state.bootstrap_start_state IN ('started', 'uncertain') AND state.resources_bound = 0 AND state.jit_state = 'not_requested' AND state.publication_state = 'not_requested' AND state.delivery_state = 'not_requested' AND state.storage_cleanup_proven = 0";

async fn single(journal: &Journal, sql: &str, name: &str) -> TestResult<String> {
    let connection = journal.connection().await?;
    let mut rows = connection.query(sql, [name]).await?;
    Ok(rows.next().await?.ok_or("row missing")?.get::<String>(0)?)
}

async fn downgrade_to_v43(journal: &Journal) -> TestResult<()> {
    let connection = journal.connection().await?;
    for name in GUARDS {
        let sql = single(
            journal,
            "SELECT sql FROM sqlite_schema WHERE name = ?1",
            name,
        )
        .await?;
        let start = sql.find(V44_SHAPE_START).ok_or("shape start missing")?;
        let end = sql.find(V44_SHAPE_END).ok_or("shape end missing")? + V44_SHAPE_END.len();
        let older = format!("{}{V43_SHAPE}{}", &sql[..start], &sql[end..]);
        connection
            .execute(&format!("DROP TRIGGER {name}"), ())
            .await?;
        connection.execute(&older, ()).await?;
    }
    connection.execute("PRAGMA user_version = 43", ()).await?;
    Ok(())
}

async fn version(journal: &Journal) -> TestResult<i64> {
    let connection = journal.connection().await?;
    let mut rows = connection.query("PRAGMA user_version", ()).await?;
    Ok(rows.next().await?.ok_or("version missing")?.get::<i64>(0)?)
}

async fn dump(journal: &Journal) -> TestResult<Vec<String>> {
    let connection = journal.connection().await?;
    let mut dumped = Vec::new();
    for table in TABLES {
        let mut rows = connection
            .query(&format!("SELECT * FROM {table}"), ())
            .await?;
        while let Some(row) = rows.next().await? {
            let values = (0..row.column_count())
                .map(|index| row.get_value(index).map(|value| format!("{value:?}")))
                .collect::<Result<Vec<_>, _>>()?;
            dumped.push(format!("{table}: {}", values.join(", ")));
        }
    }
    dumped.sort();
    Ok(dumped)
}

#[tokio::test]
async fn a_v43_journal_with_in_flight_intents_migrates_to_v48_unchanged() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("launch.db");
    let journal = Journal::open_historical_v45_for_test(&path).await?;
    let pending = journal.begin("launch", "pending-subject").await?;
    let uncertain = journal.begin("launch", "uncertain-subject").await?;
    journal.finish(uncertain, Outcome::Uncertain).await?;
    downgrade_to_v43(&journal).await?;
    assert_eq!(version(&journal).await?, 43);
    let before = dump(&journal).await?;
    assert_eq!(before.len(), 2);
    drop(journal);

    let journal = Journal::open(&path).await?;

    assert_eq!(version(&journal).await?, 48);
    assert_eq!(dump(&journal).await?, before);
    assert_eq!(journal.read(pending).await?, IntentState::Pending);
    assert_eq!(journal.read(uncertain).await?, IntentState::Uncertain);
    Ok(())
}

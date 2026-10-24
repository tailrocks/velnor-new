use crate::{Journal, Outcome, launch_harness::Scratch};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn capacity_rows_match_slot_holds_and_ignore_terminal_history() -> TestResult {
    let scratch = Scratch::new("capacity-active-rows")?;
    let journal = Journal::open(&scratch.file()).await?;
    for index in 0..64 {
        let subject = format!("history-{index}");
        let (row, _) = journal.begin_launch(&subject).await?;
        journal.finish(row, Outcome::Done).await?;
        journal.record_cleanup(row).await?;
    }
    let (uncertain, _) = journal.begin_launch("m-active-uncertain").await?;
    journal.finish(uncertain, Outcome::Uncertain).await?;
    let (failed, _) = journal.begin_launch("m-failed-owned-runner").await?;
    journal.finish(failed, Outcome::DefiniteFailure).await?;
    journal.bind(failed, None, Some("runner-73")).await?;
    let acquire = journal.begin("acquire", "failed-acquire").await?;
    journal.finish(acquire, Outcome::DefiniteFailure).await?;

    let database = turso::Builder::new_local(scratch.file().to_str().ok_or("journal path")?)
        .build()
        .await?;
    let connection = database.connect()?;
    let query = format!(
        "SELECT id FROM intents WHERE {} ORDER BY id",
        crate::launch::HOLDS_ROWS_SQL
    );
    let mut rows = connection.query(&query, ()).await?;
    let mut expected = Vec::new();
    while let Some(row) = rows.next().await? {
        expected.push(row.get::<i64>(0)?);
    }
    drop(rows);
    drop(connection);
    drop(database);
    let active = journal.capacity_rows(2).await?;
    let actual: Vec<_> = active.iter().map(|row| row.id).collect();

    assert_eq!(expected, [uncertain, failed]);
    assert_eq!(actual, expected);
    assert!(journal.capacity_rows(1).await.is_err());
    Ok(())
}

#[tokio::test]
async fn capacity_snapshot_does_not_filter_unknown_active_state() -> TestResult {
    let scratch = Scratch::new("capacity-unknown-active-state")?;
    let journal = Journal::open(&scratch.file()).await?;
    let (row, _) = journal.begin_launch("m-unknown-state").await?;
    let database = turso::Builder::new_local(scratch.file().to_str().ok_or("journal path")?)
        .build()
        .await?;
    let connection = database.connect()?;
    connection
        .execute("UPDATE intents SET state = 'unknown' WHERE id = ?1", [row])
        .await?;
    drop(connection);
    drop(database);

    assert!(journal.capacity_rows(1).await.is_err());
    Ok(())
}

use super::*;
use crate::Outcome;

#[tokio::test]
async fn an_idless_row_without_a_durable_volume_blocks_new_capacity() -> Result<(), String> {
    let (_scratch, journal) = journal("resource-idless-row").await?;
    journal
        .bind_engine(ENGINE)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .begin_launch("m103")
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![http(200, &docker_info(18, 128 * GIB, ENGINE))])?;

    let capacity = discover_after(
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_secs(1),
    )
    .await;
    stub.finish().await?;

    assert_eq!(capacity, Discovery::Unavailable);
    Ok(())
}

#[tokio::test]
async fn safe_idless_uncertain_row_is_charged_one_current_pair() -> Result<(), String> {
    let (_scratch, journal) = journal("resource-safe-idless-row").await?;
    journal
        .bind_engine(ENGINE)
        .await
        .map_err(|error| error.to_string())?;
    let (row, _) = journal
        .begin_launch("m106")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![http(200, &docker_info(18, 128 * GIB, ENGINE))])?;

    let capacity = discover_after(
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_secs(1),
    )
    .await;
    let requests = stub.finish().await?;

    assert_eq!(
        capacity,
        Discovery::Available(JobCapacity {
            total: 4,
            pair_fits: true
        })
    );
    assert_eq!(requests.len(), 1);
    Ok(())
}

#[tokio::test]
async fn safe_idless_redelivery_keeps_occupied_pair_and_zero_fit_denies_starts()
-> Result<(), String> {
    let (_scratch, journal) = journal("resource-idless-zero-fit").await?;
    journal
        .bind_engine(ENGINE)
        .await
        .map_err(|error| error.to_string())?;
    let (row, _) = journal
        .begin_launch("m108")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![http(
        200,
        &docker_info(1, budget.pair().memory_bytes, ENGINE),
    )])?;

    let discovery = discover_after(&stub.docker, &journal, budget, 8, Duration::from_secs(1)).await;
    let requests = stub.finish().await?;

    let Discovery::Available(capacity) = discovery else {
        return Err("trusted engine should preserve the measured no-fit capacity".to_owned());
    };
    assert_eq!(capacity.total(), 1);
    assert!(!capacity.permits_start());
    assert_eq!(requests.len(), 1);
    Ok(())
}

#[tokio::test]
async fn attempted_idless_uncertain_row_stays_capacity_unavailable() -> Result<(), String> {
    let (_scratch, journal) = journal("resource-attempted-idless-row").await?;
    journal
        .bind_engine(ENGINE)
        .await
        .map_err(|error| error.to_string())?;
    let (row, _) = journal
        .begin_assigned_launch("m107r61", 1, 61, "v61")
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .claim_assigned_acquire(row)
            .await
            .map_err(|error| error.to_string())?
    );
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![http(200, &docker_info(18, 128 * GIB, ENGINE))])?;

    let capacity = discover_after(
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_secs(1),
    )
    .await;
    let requests = stub.finish().await?;

    assert_eq!(capacity, Discovery::Unavailable);
    assert_eq!(requests.len(), 1);
    Ok(())
}

#[tokio::test]
async fn already_clean_rows_do_not_consume_capacity() -> Result<(), String> {
    let (_scratch, journal) = journal("resource-clean-row").await?;
    journal
        .bind_engine(ENGINE)
        .await
        .map_err(|error| error.to_string())?;
    let (row, _) = journal
        .begin_launch("m104")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_cleanup(row)
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![http(200, &docker_info(18, 128 * GIB, ENGINE))])?;

    let capacity = discover_after(
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_secs(1),
    )
    .await;
    stub.finish().await?;

    assert_eq!(
        capacity,
        Discovery::Available(JobCapacity {
            total: 4,
            pair_fits: true
        })
    );
    Ok(())
}

#[tokio::test]
async fn capacity_rows_match_slot_holds_and_ignore_terminal_history() -> Result<(), String> {
    let (_scratch, journal) = journal("resource-capacity-active-rows").await?;
    for index in 0..64 {
        let subject = format!("history-{index}");
        let (row, _) = journal
            .begin_launch(&subject)
            .await
            .map_err(|error| error.to_string())?;
        journal
            .finish(row, Outcome::Done)
            .await
            .map_err(|error| error.to_string())?;
        journal
            .record_cleanup(row)
            .await
            .map_err(|error| error.to_string())?;
    }
    let (uncertain, _) = journal
        .begin_launch("m-active-uncertain")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(uncertain, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let (failed, _) = journal
        .begin_launch("m-failed-owned-runner")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(failed, Outcome::DefiniteFailure)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind(failed, None, Some("runner-73"))
        .await
        .map_err(|error| error.to_string())?;
    let acquire = journal
        .begin("acquire", "failed-acquire")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(acquire, Outcome::DefiniteFailure)
        .await
        .map_err(|error| error.to_string())?;

    let all = journal.rows().await.map_err(|error| error.to_string())?;
    let expected: Vec<_> = all
        .iter()
        .filter(|row| crate::launch::slot::holds(row))
        .map(|row| row.id)
        .collect();
    let active = journal
        .capacity_rows(2)
        .await
        .map_err(|error| error.to_string())?;
    let actual: Vec<_> = active.iter().map(|row| row.id).collect();

    assert_eq!(expected, [uncertain, failed]);
    assert_eq!(actual, expected);
    assert!(journal.capacity_rows(1).await.is_err());
    Ok(())
}

#[tokio::test]
async fn capacity_snapshot_does_not_filter_unknown_active_state() -> Result<(), String> {
    let (scratch, journal) = journal("resource-unknown-active-state").await?;
    let (row, _) = journal
        .begin_launch("m-unknown-state")
        .await
        .map_err(|error| error.to_string())?;
    let path = scratch
        .file()
        .to_str()
        .ok_or_else(|| "journal path is not UTF-8".to_owned())?
        .to_owned();
    let database = turso::Builder::new_local(&path)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = database.connect().map_err(|error| error.to_string())?;
    connection
        .execute("UPDATE intents SET state = 'unknown' WHERE id = ?1", [row])
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(database);

    assert!(journal.capacity_rows(1).await.is_err());
    Ok(())
}

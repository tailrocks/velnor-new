use crate::error::HostError;
use crate::journal::{Journal, request_lifecycle::tests::native_started_intake_fixture};

#[tokio::test]
async fn v32_shaped_migration_preserves_live_link_as_current_attempt() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let path = fixture.database_path.clone();
    let reservation_id = fixture.reservation_id;
    let intent_id = fixture.intent_id;
    let initial_attempt = fixture
        .scope
        .current_physical_attempt(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(initial_attempt.reservation_id, reservation_id);
    assert_eq!(initial_attempt.intent_id, intent_id);
    assert_eq!(initial_attempt.generation, 1);
    assert_eq!(initial_attempt.witness_request_id, fixture.request_id);
    let connection = fixture.journal.connection().await?;
    for table in [
        "physical_attempt_outcomes",
        "physical_attempt_current",
        "physical_attempts",
        "physical_attempt_lineages",
    ] {
        connection
            .execute(&format!("DROP TABLE {table}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    super::storage_network::restore_v32_transition_for_test(&connection).await?;
    connection
        .execute("PRAGMA user_version = 32", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture { _scratch, .. } =
        fixture;

    let reopened = Journal::open(&path).await?;
    let connection = reopened.connection().await?;
    let mut rows = connection
        .query(
            "SELECT attempts.lineage_id, attempts.generation, attempts.reservation_id, attempts.intent_id, attempts.witness_request_id, attempts.physical_nonce, current.reservation_id FROM physical_attempts AS attempts JOIN physical_attempt_current AS current ON current.lineage_id = attempts.lineage_id AND current.generation = attempts.generation WHERE attempts.reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows.next().await.map_err(|_| HostError::Journal)?;
    let row = row.ok_or(HostError::Journal)?;
    let lineage_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let generation: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let migrated_reservation_id: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    let migrated_intent_id: i64 = row.get(3).map_err(|_| HostError::Journal)?;
    let witness_request_id: i64 = row.get(4).map_err(|_| HostError::Journal)?;
    let nonce: String = row.get(5).map_err(|_| HostError::Journal)?;
    let current_reservation_id: i64 = row.get(6).map_err(|_| HostError::Journal)?;
    assert!(rows.next().await.map_err(|_| HostError::Journal)?.is_none());
    assert!(lineage_id > 0);
    assert_eq!(generation, 1);
    assert_eq!(migrated_reservation_id, reservation_id);
    assert_eq!(migrated_intent_id, intent_id);
    assert_eq!(current_reservation_id, reservation_id);
    assert_eq!(witness_request_id, 101);
    assert_eq!(nonce.len(), 32);
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 33);
    let link_count = scalar(
        &connection,
        "SELECT COUNT(*) FROM connection_request_reservations",
    )
    .await?;
    assert_eq!(link_count, 1);
    Ok(())
}

async fn scalar(connection: &turso::Connection, sql: &str) -> Result<i64, HostError> {
    let mut rows = connection
        .query(sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

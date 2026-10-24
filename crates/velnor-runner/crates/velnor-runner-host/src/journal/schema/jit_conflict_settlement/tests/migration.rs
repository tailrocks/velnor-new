use crate::error::HostError;
use crate::journal::request_lifecycle::tests::helpers::{
    Scratch, binding, created_scope, provision_acquired_request_with_initialization,
};
use crate::journal::{CapacityLimits, Journal, ReservationState};

#[tokio::test]
async fn v39_migration_opens_and_reopens_with_current_contract() -> Result<(), HostError> {
    Box::pin(v39_migration_opens_and_reopens_with_current_contract_inner()).await
}

async fn v39_migration_opens_and_reopens_with_current_contract_inner() -> Result<(), HostError> {
    let fixture = super::v39_fixture().await?;
    let original_trigger_order = affected_trigger_order(fixture.connection()).await?;
    let path = fixture.path().to_owned();
    let _scratch = fixture.close();

    let migrated = Journal::open(&path).await?;
    let connection = migrated.connection().await?;
    assert_eq!(
        scalar(&connection, "PRAGMA user_version").await?,
        super::super::super::JOURNAL_VERSION
    );
    super::super::super::validate_persisted_schema_v40(&connection).await?;
    assert_eq!(
        affected_trigger_order(&connection).await?,
        original_trigger_order
    );
    drop(connection);
    drop(migrated);

    let reopened = Journal::open(&path).await?;
    let connection = reopened.connection().await?;
    assert_eq!(
        scalar(&connection, "PRAGMA user_version").await?,
        super::super::super::JOURNAL_VERSION
    );
    super::super::super::validate_persisted_schema_v40(&connection).await?;
    assert_eq!(
        affected_trigger_order(&connection).await?,
        original_trigger_order
    );
    Ok(())
}

#[tokio::test]
async fn v39_migration_preserves_historical_uncertain_jit_charge() -> Result<(), HostError> {
    Box::pin(v39_migration_preserves_historical_uncertain_jit_charge_inner()).await
}

async fn v39_migration_preserves_historical_uncertain_jit_charge_inner() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let repository = binding("jit-v39-preserve", 101)?;
    let scope = created_scope(&journal, &repository, 501).await?;
    let physical = provision_acquired_request_with_initialization(&scope, 72, 'd', 'd').await?;
    let requested_name = format!("v{}", "d".repeat(32));
    let (authority_generation, authority_digest) = scope.test_active_authority_identity().await?;
    let claim = scope
        .claim_runner_jit_for_request(
            physical.intent_id,
            physical.reservation_id,
            72,
            &requested_name,
            authority_generation,
            &authority_digest,
        )
        .await?
        .ok_or(HostError::Journal)?;
    scope.mark_runner_jit_uncertain(claim).await?;
    let before = scope
        .physical_runner_reservations()
        .await?
        .into_iter()
        .find(|row| row.reservation_id == physical.reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(before.reservation.state, ReservationState::Uncertain);
    assert_eq!(format!("{:?}", before.jit_state), "Uncertain");
    drop(scope);
    drop(journal);

    Box::pin(downgrade_current_database_to_v39(&path)).await?;
    let migrated = Journal::open(&path).await?;
    let connection = migrated.connection().await?;
    assert_eq!(
        scalar(&connection, "PRAGMA user_version").await?,
        super::super::super::JOURNAL_VERSION
    );
    super::super::super::validate_persisted_schema_v40(&connection).await?;
    let reopened_scope = migrated.connection_scope(&repository).await?;
    let after = reopened_scope
        .physical_runner_reservations()
        .await?
        .into_iter()
        .find(|row| row.reservation_id == physical.reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(after, before);
    drop(reopened_scope);
    drop(connection);
    drop(migrated);

    let reopened = Journal::open(&path).await?;
    let reopened_scope = reopened.connection_scope(&repository).await?;
    assert_eq!(
        reopened_scope
            .physical_runner_reservations()
            .await?
            .into_iter()
            .find(|row| row.reservation_id == physical.reservation_id),
        Some(before)
    );
    Ok(())
}

#[tokio::test]
async fn v39_migration_failure_rolls_back_and_keeps_v39_usable() -> Result<(), HostError> {
    Box::pin(v39_migration_failure_rolls_back_and_keeps_v39_usable_inner()).await
}

async fn v39_migration_failure_rolls_back_and_keeps_v39_usable_inner() -> Result<(), HostError> {
    let fixture = super::v39_fixture().await?;
    let connection = fixture.connection();
    connection
        .execute(
            "CREATE TABLE physical_attempt_jit_conflict_recovery_progress (blocker INTEGER)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(super::super::migrate_v39_to_v40(connection).await.is_err());
    connection
        .execute("ROLLBACK", ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert_eq!(scalar(connection, "PRAGMA user_version").await?, 39);
    assert_eq!(
        scalar(
            connection,
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='physical_attempt_jit_conflict_recovery_claims'",
        )
        .await?,
        0
    );
    assert_eq!(
        scalar(
            connection,
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='physical_attempt_jit_conflict_recovery_progress'",
        )
        .await?,
        1
    );
    super::super::super::legacy_validation::validate_persisted_schema_v39(connection).await?;
    connection
        .execute(
            "DROP TABLE physical_attempt_jit_conflict_recovery_progress",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let path = fixture.path().to_owned();
    let _scratch = fixture.close();
    let migrated = Journal::open(&path).await?;
    let connection = migrated.connection().await?;
    assert_eq!(
        scalar(&connection, "PRAGMA user_version").await?,
        super::super::super::JOURNAL_VERSION
    );
    super::super::super::validate_persisted_schema_v40(&connection).await
}

#[tokio::test]
async fn null_runner_absence_settlement_survives_v40_reopen() -> Result<(), HostError> {
    Box::pin(null_runner_absence_settlement_survives_v40_reopen_inner()).await
}

async fn null_runner_absence_settlement_survives_v40_reopen_inner() -> Result<(), HostError> {
    let fixture = super::local_absence_fixture('a', 811, 1_111).await?;
    assert_eq!(
        super::settle_empty_remote_case(&fixture, 'b').await?,
        crate::journal::MetadataSettlementResult::Settled
    );
    let path = fixture.scratch.file();
    let reservation_id = fixture.physical.reservation_id;
    let name = fixture.requested_name.clone();
    let set_id = fixture.recovery_claim.scale_set_id();
    let super::LocalAbsenceFixture {
        scratch,
        journal,
        scope,
        ..
    } = fixture;
    drop(scope);
    drop(journal);

    let reopened = Journal::open(&path).await?;
    let connection = reopened.connection().await?;
    super::super::super::validate_persisted_schema_v40(&connection).await?;
    assert_eq!(
        scalar(
            &connection,
            &format!("SELECT COUNT(*) FROM physical_attempt_jit_conflict_settlements WHERE reservation_id={reservation_id} AND remote_runner_id IS NULL AND remote_runner_name='{name}' AND remote_scale_set_id={set_id}"),
        ).await?,
        1
    );
    assert_eq!(
        scalar(
            &connection,
            &format!("SELECT COUNT(*) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id={reservation_id} AND phase_index IN (2,4) AND runner_id IS NULL"),
        ).await?,
        2
    );
    assert_eq!(
        scalar(
            &connection,
            &format!("SELECT COUNT(*) FROM connection_runner_jits WHERE requested_name='{name}' AND state='conflict_retired' AND runner_id IS NULL AND runner_name IS NULL"),
        ).await?,
        1
    );
    assert_eq!(
        scalar(
            &connection,
            &format!("SELECT COUNT(*) FROM capacity_reservations WHERE id={reservation_id} AND state='released' AND remote_cleanup_proven=1 AND local_cleanup_proven=1"),
        ).await?,
        1
    );
    drop(connection);
    drop(reopened);
    drop(scratch);
    Ok(())
}

pub(in crate::journal::schema) async fn downgrade_current_database_to_v39(
    path: &std::path::Path,
) -> Result<(), HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    downgrade_connection_to_v39(&connection).await?;
    drop(connection);
    drop(database);
    Ok(())
}

pub(in crate::journal::schema) async fn downgrade_connection_to_v39(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    // Reuse real v39 definitions so retained charge data crosses the production migration.
    let old = super::v39_fixture().await?;
    let old_connection = old.connection();
    let legacy_metadata_guard =
        super::super::super::provisioning::metadata_settlement_claim_guard_sql()
            .ok_or(HostError::Journal)?;
    let table_sql = super::object_sql(old_connection, "table", "connection_runner_jits").await?;
    let view_sql =
        super::object_sql(old_connection, "view", "physical_attempt_terminal_proofs").await?;
    let indexes = super::attached_sql(old_connection, "index", "connection_runner_jits").await?;
    let triggers = super::attached_sql(old_connection, "trigger", "connection_runner_jits").await?;
    for table in super::super::table_names() {
        let count = scalar(connection, &format!("SELECT COUNT(*) FROM {table}")).await?;
        if count != 0 {
            return Err(HostError::Journal);
        }
    }
    for (kind, name, _) in super::super::schema_objects() {
        if kind == "trigger" {
            super::drop_downgrade_object(connection, "TRIGGER", name).await?;
        }
    }
    super::drop_downgrade_object(
        connection,
        "TRIGGER",
        "metadata_settlement_claim_insert_guard",
    )
    .await?;
    for table in super::super::table_names() {
        for suffix in ["insert", "update", "delete"] {
            let name = format!("{table}_revision_{suffix}");
            super::drop_downgrade_object(connection, "TRIGGER", &name).await?;
        }
    }
    let legacy_external_trigger_sql =
        super::suspend_external_jit_triggers_for_downgrade(old_connection, connection).await?;
    let legacy_terminal_view_trigger_sql =
        super::suspend_terminal_view_triggers_for_downgrade(old_connection, connection).await?;
    let _old_scratch = old.close();
    super::drop_downgrade_object(connection, "VIEW", "physical_attempt_terminal_proofs").await?;
    for table in super::super::table_names().iter().rev() {
        super::drop_downgrade_object(connection, "TABLE", table).await?;
    }
    super::replace_jit_table_for_downgrade(connection, &table_sql).await?;
    for sql in indexes.into_iter().chain(triggers) {
        connection
            .execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(&view_sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::restore_terminal_view_triggers(connection, &legacy_terminal_view_trigger_sql).await?;
    super::restore_external_jit_triggers(connection, &legacy_external_trigger_sql).await?;
    connection
        .execute(legacy_metadata_guard, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version=39", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::super::super::legacy_validation::validate_persisted_schema_v39(connection).await?;
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

async fn affected_trigger_order(
    connection: &turso::Connection,
) -> Result<Vec<(String, String, String)>, HostError> {
    let tables = super::super::migration::REBUILT_TRIGGER_TABLES
        .iter()
        .map(|table| format!("'{table}'"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT tbl_name,name,sql FROM sqlite_schema WHERE type='trigger' AND tbl_name IN ({tables}) ORDER BY tbl_name,rowid"
    );
    let mut rows = connection
        .query(&sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut triggers = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let sql = row.get::<String>(2).map_err(|_| HostError::Journal)?;
        triggers.push((
            row.get::<String>(0).map_err(|_| HostError::Journal)?,
            row.get::<String>(1).map_err(|_| HostError::Journal)?,
            super::super::super::sql_normalization::normalize(&sql),
        ));
    }
    let added_v40_triggers = super::super::schema_objects()
        .filter(|(kind, _, _)| *kind == "trigger")
        .map(|(_, name, _)| name)
        .collect::<Vec<_>>();
    triggers.retain(|(_, name, _)| !added_v40_triggers.contains(&name.as_str()));
    Ok(triggers)
}

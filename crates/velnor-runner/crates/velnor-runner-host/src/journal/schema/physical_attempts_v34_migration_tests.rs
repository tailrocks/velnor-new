use crate::error::HostError;
use crate::journal::{Journal, request_lifecycle::tests::native_started_intake_fixture};

#[tokio::test]
async fn v34_schema_migrates_to_current_schema_and_reopens_without_changing_attempt_rows()
-> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let path = fixture.database_path.clone();
    let reservation_id = fixture.reservation_id;
    let initial = fixture
        .scope
        .current_physical_attempt(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let initial_identity = (
        initial.lineage_id,
        initial.generation,
        initial.intent_id,
        initial.physical_nonce.clone(),
    );
    let connection = fixture.journal.connection().await?;
    downgrade_to_v34(&connection).await?;
    drop(connection);
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture { _scratch, .. } =
        fixture;

    let migrated = Journal::open(&path).await?;
    let connection = migrated.connection().await?;
    assert_eq!(
        scalar(&connection, "PRAGMA user_version").await?,
        super::JOURNAL_VERSION
    );
    super::physical_attempts::validate_v35(&connection).await?;
    super::post_jit_effect_fences::validate(&connection).await?;
    assert_attempt_matches(&connection, reservation_id, &initial_identity).await?;
    drop(connection);
    drop(migrated);

    let reopened = Journal::open(&path).await?;
    let connection = reopened.connection().await?;
    assert_eq!(
        scalar(&connection, "PRAGMA user_version").await?,
        super::JOURNAL_VERSION
    );
    super::physical_attempts::validate_v35(&connection).await?;
    super::post_jit_effect_fences::validate(&connection).await?;
    assert_attempt_matches(&connection, reservation_id, &initial_identity).await
}

#[tokio::test]
async fn v34_upgrade_rolls_back_the_new_schema_with_the_outer_transaction() -> Result<(), HostError>
{
    let fixture = native_started_intake_fixture().await?;
    let path = fixture.database_path.clone();
    let connection = fixture.journal.connection().await?;
    downgrade_to_v34(&connection).await?;

    connection
        .execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::migration::from_version_thirty_four(&connection).await?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 35);
    assert!(
        connection
            .execute("INSERT INTO missing_after_v35_migration VALUES (1)", ())
            .await
            .is_err()
    );
    connection
        .execute("ROLLBACK", ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 34);
    super::validate_persisted_schema_v34(&connection).await?;
    drop(connection);
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture { _scratch, .. } =
        fixture;

    let migrated = Journal::open(&path).await?;
    let connection = migrated.connection().await?;
    assert_eq!(
        scalar(&connection, "PRAGMA user_version").await?,
        super::JOURNAL_VERSION
    );
    super::physical_attempts::validate_v35(&connection).await?;
    super::post_jit_effect_fences::validate(&connection).await?;
    super::post_jit_retirement::validate_v40(&connection).await
}

#[tokio::test]
async fn v35_open_rejects_a_changed_terminal_proof_view() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let path = fixture.database_path.clone();
    let connection = fixture.journal.connection().await?;
    connection
        .execute("DROP VIEW physical_attempt_terminal_proofs", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE VIEW physical_attempt_terminal_proofs AS SELECT lineage_id, generation, reservation_id FROM physical_attempts",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture { _scratch, .. } =
        fixture;

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

pub(super) async fn downgrade_to_v34(connection: &turso::Connection) -> Result<(), HostError> {
    super::jit_conflict_settlement::downgrade_connection_to_v39(connection).await?;
    super::jit_conflict_recovery::remove_receipt_schema_for_migration_test(connection).await?;
    for trigger in [
        "post_jit_jit_claim_fence",
        "post_jit_runner_start_fence",
        "post_jit_reservation_jit_fence",
        "post_jit_job_facts_fence",
        "post_jit_listener_fence",
        "post_jit_challenge_fence",
        "post_jit_decision_fence",
        "post_jit_publication_fence",
    ] {
        connection
            .execute(&format!("DROP TRIGGER {trigger}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute("DROP TRIGGER physical_attempt_insert_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TRIGGER physical_attempt_current_delete_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(super::physical_attempts::ATTEMPT_INSERT_GUARD, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(super::physical_attempts::CURRENT_DELETE_GUARD, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TRIGGER physical_attempt_outcome_retirement_fence", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP VIEW physical_attempt_terminal_proofs", ())
        .await
        .map_err(|_| HostError::Journal)?;
    for table in [
        "physical_attempt_post_jit_retirements",
        "physical_attempt_post_jit_retirement_remote_absence",
        "physical_attempt_post_jit_retirement_progress",
        "physical_attempt_post_jit_retirement_claims",
    ] {
        connection
            .execute(&format!("DROP TABLE {table}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute("PRAGMA user_version = 34", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate_persisted_schema_v34(connection).await
}

async fn assert_attempt_matches(
    connection: &turso::Connection,
    reservation_id: i64,
    initial: &(i64, i64, i64, String),
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT attempt.lineage_id, attempt.generation, attempt.intent_id, attempt.physical_nonce FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id=current.lineage_id AND attempt.generation=current.generation AND attempt.reservation_id=current.reservation_id AND attempt.intent_id=current.intent_id WHERE current.reservation_id=?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        row.get::<i64>(0).map_err(|_| HostError::Journal)?,
        initial.0
    );
    assert_eq!(
        row.get::<i64>(1).map_err(|_| HostError::Journal)?,
        initial.1
    );
    assert_eq!(
        row.get::<i64>(2).map_err(|_| HostError::Journal)?,
        initial.2
    );
    assert_eq!(
        row.get::<String>(3).map_err(|_| HostError::Journal)?,
        initial.3
    );
    assert!(rows.next().await.map_err(|_| HostError::Journal)?.is_none());
    Ok(())
}

#[tokio::test]
async fn v33_outcome_guard_migrates_without_changing_attempt_rows() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let path = fixture.database_path.clone();
    let reservation_id = fixture.reservation_id;
    let intent_id = fixture.intent_id;
    let initial = fixture
        .scope
        .current_physical_attempt(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let connection = fixture.journal.connection().await?;
    downgrade_to_v34(&connection).await?;
    connection
        .execute("DROP TRIGGER physical_attempt_outcome_insert_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(super::physical_attempts::OUTCOME_INSERT_GUARD_V33, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 33", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::physical_attempts::validate_v33(&connection).await?;
    super::validate_persisted_schema_v33(&connection).await?;
    drop(connection);
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture { _scratch, .. } =
        fixture;

    let migrated = Journal::open(&path).await?;
    let connection = migrated.connection().await?;
    assert_eq!(
        scalar(&connection, "PRAGMA user_version").await?,
        super::JOURNAL_VERSION
    );
    super::physical_attempts::validate_v35(&connection).await?;
    super::post_jit_effect_fences::validate(&connection).await?;
    super::post_jit_retirement::validate_v40(&connection).await?;
    let mut rows = connection
        .query(
            "SELECT attempt.lineage_id, attempt.generation, attempt.reservation_id, attempt.intent_id, attempt.physical_nonce FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id WHERE current.reservation_id = ?1",
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
    let nonce: String = row.get(4).map_err(|_| HostError::Journal)?;
    assert!(rows.next().await.map_err(|_| HostError::Journal)?.is_none());
    assert_eq!(lineage_id, initial.lineage_id);
    assert_eq!(generation, initial.generation);
    assert_eq!(migrated_reservation_id, reservation_id);
    assert_eq!(migrated_intent_id, intent_id);
    assert_eq!(nonce, initial.physical_nonce);
    assert_eq!(
        scalar(
            &connection,
            "SELECT COUNT(*) FROM physical_attempt_outcomes"
        )
        .await?,
        0
    );
    drop(connection);
    drop(migrated);

    let reopened = Journal::open(&path).await?;
    let connection = reopened.connection().await?;
    assert_eq!(
        scalar(&connection, "PRAGMA user_version").await?,
        super::JOURNAL_VERSION
    );
    super::physical_attempts::validate_v35(&connection).await?;
    super::post_jit_effect_fences::validate(&connection).await?;
    super::post_jit_retirement::validate_v40(&connection).await
}

#[tokio::test]
async fn v33_migration_rejects_the_v34_guard_without_changing_the_version() -> Result<(), HostError>
{
    let fixture = native_started_intake_fixture().await?;
    let journal = fixture.journal.clone();
    let path = fixture.database_path.clone();
    let connection = journal.connection().await?;
    downgrade_to_v34(&connection).await?;
    connection
        .execute("PRAGMA user_version = 33", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture { _scratch, .. } =
        fixture;

    assert!(matches!(
        Journal::open(&path).await,
        Err(HostError::Journal)
    ));
    let connection = journal.connection().await?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 33);
    assert_eq!(
        scalar(
            &connection,
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'trigger' AND name = 'physical_attempt_outcome_insert_guard'"
        )
        .await?,
        1
    );
    super::physical_attempts::validate(&connection).await
}

#[tokio::test]
async fn v33_guard_replacement_rolls_back_with_the_outer_migration_transaction()
-> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let connection = fixture.journal.connection().await?;
    downgrade_to_v34(&connection).await?;
    connection
        .execute("DROP TRIGGER physical_attempt_outcome_insert_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(super::physical_attempts::OUTCOME_INSERT_GUARD_V33, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 33", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate_persisted_schema_v33(&connection).await?;

    connection
        .execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::migration::from_version_thirty_three(&connection).await?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 34);
    assert!(
        connection
            .execute(
                "INSERT INTO table_missing_after_trigger_replacement VALUES (1)",
                ()
            )
            .await
            .is_err()
    );
    connection
        .execute("ROLLBACK", ())
        .await
        .map_err(|_| HostError::Journal)?;

    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 33);
    super::validate_persisted_schema_v33(&connection).await?;
    drop(connection);
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture { _scratch, .. } =
        fixture;
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

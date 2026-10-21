use crate::error::HostError;
use crate::journal::{Journal, request_lifecycle::tests::native_started_intake_fixture};

const FENCE_NAMES: [&str; 8] = [
    "post_jit_jit_claim_fence",
    "post_jit_runner_start_fence",
    "post_jit_reservation_jit_fence",
    "post_jit_job_facts_fence",
    "post_jit_listener_fence",
    "post_jit_challenge_fence",
    "post_jit_decision_fence",
    "post_jit_publication_fence",
];

#[tokio::test]
async fn v35_migration_installs_fences_atomically_and_reopens() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let path = fixture.database_path.clone();
    let connection = fixture.journal.connection().await?;
    drop_fences(&connection).await?;
    install_v36_retirement_guard(&connection).await?;
    connection
        .execute("PRAGMA user_version=35", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate_persisted_schema_v35(&connection).await?;

    connection
        .execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::migration::from_version_thirty_five(&connection).await?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 36);
    super::post_jit_effect_fences::validate(&connection).await?;
    connection
        .execute("ROLLBACK", ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 35);
    assert_fences_absent(&connection).await?;
    super::validate_persisted_schema_v35(&connection).await?;
    drop(connection);
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture { _scratch, .. } =
        fixture;

    let migrated = Journal::open(&path).await?;
    let connection = migrated.connection().await?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 37);
    super::post_jit_effect_fences::validate(&connection).await?;
    super::post_jit_retirement::validate_v37(&connection).await?;
    drop(connection);
    drop(migrated);

    let reopened = Journal::open(&path).await?;
    let connection = reopened.connection().await?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 37);
    super::post_jit_effect_fences::validate(&connection).await?;
    super::post_jit_retirement::validate_v37(&connection).await
}

#[tokio::test]
async fn current_open_rejects_a_changed_executable_fence() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let path = fixture.database_path.clone();
    fixture
        .journal
        .connection()
        .await?
        .execute("DROP TRIGGER post_jit_listener_fence", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture { _scratch, .. } =
        fixture;

    assert!(matches!(
        Journal::open(&path).await,
        Err(HostError::Journal)
    ));
    Ok(())
}

async fn drop_fences(connection: &turso::Connection) -> Result<(), HostError> {
    for name in FENCE_NAMES {
        connection
            .execute(&format!("DROP TRIGGER {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn install_v36_retirement_guard(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(
            "DROP TRIGGER physical_attempt_post_jit_retirement_claim_insert_guard",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(super::post_jit_retirement::CLAIM_INSERT_GUARD_V36, ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)
}

async fn assert_fences_absent(connection: &turso::Connection) -> Result<(), HostError> {
    for name in FENCE_NAMES {
        let count = scalar(
            connection,
            &format!("SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name='{name}'"),
        )
        .await?;
        assert_eq!(
            count, 0,
            "The database trigger must not exist after ROLLBACK."
        );
    }
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

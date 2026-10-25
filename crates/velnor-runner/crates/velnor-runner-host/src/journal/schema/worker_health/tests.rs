use crate::error::HostError;
use crate::journal::{Journal, request_lifecycle::tests::native_started_intake_fixture};

#[tokio::test]
async fn v40_migration_keeps_live_physical_charge_and_starts_with_empty_health()
-> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let path = fixture.database_path.clone();
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 1);
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        _scratch,
        journal,
        scope,
        ..
    } = fixture;
    let connection = journal.connection().await?;
    connection
        .execute("DROP TABLE local_worker_exits", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 40", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(scope);
    drop(journal);

    let reopened = Journal::open(&path).await?;
    assert_eq!(reopened.occupied_reservation_count().await?, 1);
    assert_eq!(
        reopened
            .connection_scope(&crate::journal::request_lifecycle::tests::helpers::binding(
                "native-started-intake",
                101,
            )?)
            .await?
            .unresolved_local_worker_exits()
            .await?,
        Vec::new()
    );
    Ok(())
}

#[tokio::test]
async fn malformed_v40_health_schema_fails_without_partial_migration() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let path = fixture.database_path.clone();
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        _scratch,
        journal,
        scope,
        ..
    } = fixture;
    let connection = journal.connection().await?;
    connection
        .execute("DROP TABLE local_worker_exits", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TRIGGER local_worker_exit_insert_guard BEFORE INSERT ON connection_scopes BEGIN SELECT 1; END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 40", ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(
        super::super::validate_persisted_schema_v40(&connection)
            .await
            .is_ok()
    );
    let trigger = object_sql_by_kind(&connection, "trigger", "local_worker_exit_insert_guard")
        .await?
        .ok_or(HostError::Journal)?;
    drop(connection);
    drop(scope);
    drop(journal);

    assert!(Journal::open(&path).await.is_err());
    let (_database, connection) = open_raw(&path).await?;
    assert_eq!(version(&connection).await?, 40);
    assert_eq!(
        object_sql_by_kind(&connection, "table", "local_worker_exits").await?,
        None
    );
    assert_eq!(
        object_sql_by_kind(
            &connection,
            "index",
            "local_worker_exits_connection_observed"
        )
        .await?,
        None
    );
    assert_eq!(
        object_sql_by_kind(&connection, "trigger", "local_worker_exit_insert_guard").await?,
        Some(trigger)
    );
    Ok(())
}

#[tokio::test]
async fn unknown_version_fails_without_schema_mutation() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let path = fixture.database_path.clone();
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        _scratch,
        journal,
        scope,
        ..
    } = fixture;
    let connection = journal.connection().await?;
    let before = object_sql(&connection, "local_worker_exits").await?;
    connection
        .execute("PRAGMA user_version = 42", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(scope);
    drop(journal);

    assert!(Journal::open(&path).await.is_err());
    let (_database, connection) = open_raw(&path).await?;
    assert_eq!(version(&connection).await?, 42);
    assert_eq!(object_sql(&connection, "local_worker_exits").await?, before);
    Ok(())
}

async fn open_raw(
    path: &std::path::Path,
) -> Result<(turso::Database, turso::Connection), HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    Ok((database, connection))
}

async fn version(connection: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = connection
        .query("PRAGMA user_version", ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn object_sql(connection: &turso::Connection, name: &str) -> Result<String, HostError> {
    object_sql_by_kind(connection, "table", name)
        .await?
        .ok_or(HostError::Journal)
}

async fn object_sql_by_kind(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
) -> Result<Option<String>, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
            (kind, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some(row.get(0).map_err(|_| HostError::Journal)?))
}

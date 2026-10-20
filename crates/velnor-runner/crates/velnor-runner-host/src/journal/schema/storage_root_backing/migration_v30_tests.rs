use crate::error::HostError;

use super::super::super::provisioning::{
    BOUND_STORAGE_ATTACHMENTS, V29_BOUND_STORAGE_ATTACHMENTS,
    restore_bound_storage_attachment_guards,
};
use super::super::{ATTACHMENT_PATHS, TABLE, TRIGGERS};
use super::super::{LEGACY_ATTACHMENT_PATHS, V29_ATTACHMENT_PATHS};

#[tokio::test]
async fn v29_storage_path_migration_preserves_rows_guards_and_foreign_keys() -> Result<(), HostError>
{
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "velnor-storage-path-v30-{}-{serial}.db",
        std::process::id()
    ));
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    create_v29_rows(&connection).await?;
    super::super::upgrade_v27(&connection).await?;
    assert_migrated_schema(&connection).await?;
    assert_dind_path_contract(&connection).await?;
    Ok(())
}

#[tokio::test]
async fn v29_attachment_with_legacy_paths_repairs_before_v30() -> Result<(), HostError> {
    let path = std::env::temp_dir().join(format!(
        "velnor-storage-path-v27-legacy-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_nanos()
    ));
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    create_v29_rows_with_paths(&connection, LEGACY_ATTACHMENT_PATHS).await?;
    super::super::upgrade_v27(&connection).await?;
    assert_migrated_schema(&connection).await?;
    assert_dind_path_contract(&connection).await?;
    Ok(())
}

async fn create_v29_rows(connection: &turso::Connection) -> Result<(), HostError> {
    create_v29_rows_with_paths(connection, V29_ATTACHMENT_PATHS).await
}

async fn create_v29_rows_with_paths(
    connection: &turso::Connection,
    path_ddl: &str,
) -> Result<(), HostError> {
    create_reservation_fixture(connection).await?;
    create_v29_storage_tables(connection, path_ddl).await?;
    seed_v29_storage_rows(connection).await?;
    restore_bound_storage_attachment_guards(connection).await?;
    for (_, sql) in TRIGGERS {
        connection
            .execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn create_reservation_fixture(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TABLE capacity_reservations (id INTEGER PRIMARY KEY, connection_id TEXT, intent_id INTEGER)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE connection_intents (id INTEGER PRIMARY KEY, state TEXT, cleanup_proven INTEGER)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE capacity_reservation_bootstrap_containers (reservation_id INTEGER, role TEXT, create_state TEXT, start_state TEXT, container_id TEXT)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE capacity_reservation_provisioning (reservation_id INTEGER PRIMARY KEY, operation_id INTEGER, engine_id TEXT, docker_root_digest TEXT, sample_generation_id TEXT)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE capacity_reservation_provisioning_state (reservation_id INTEGER PRIMARY KEY, bootstrap_start_state TEXT, resources_bound INTEGER NOT NULL)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservations VALUES (1, 'connection', 1)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_intents VALUES (1, 'pending', 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning VALUES (1, 1, 'engine', 'root', 'sample')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_bootstrap_containers VALUES (1, 'runner', 'created', 'started', 'runner-container')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning_state VALUES (1, 'started', 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn create_v29_storage_tables(
    connection: &turso::Connection,
    path_ddl: &str,
) -> Result<(), HostError> {
    connection
        .execute(V29_BOUND_STORAGE_ATTACHMENTS, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(path_ddl, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn seed_v29_storage_rows(connection: &turso::Connection) -> Result<(), HostError> {
    insert_attachment(&connection, "runner-home", "runner", "/home/runner").await?;
    insert_attachment(
        &connection,
        "engine-metadata",
        "runner",
        "/.velnor-runtime-identity",
    )
    .await?;
    insert_path(
        &connection,
        "runner-home",
        "runner",
        "/home/runner",
        "/home/runner",
        None,
    )
    .await?;
    insert_path(
        &connection,
        "engine-metadata",
        "runner",
        "/.velnor-runtime-identity",
        "/opt/velnor/init/metadata/runtime-identity",
        Some("runtime-identity"),
    )
    .await?;
    Ok(())
}

async fn assert_migrated_schema(connection: &turso::Connection) -> Result<(), HostError> {
    assert_eq!(
        count(connection, "capacity_reservation_bound_storage_attachments").await?,
        2
    );
    assert_eq!(
        count(connection, "capacity_reservation_bound_storage_paths").await?,
        2
    );
    assert_eq!(
        normalize(&table_sql(connection, "capacity_reservation_bound_storage_attachments").await?),
        normalize(BOUND_STORAGE_ATTACHMENTS)
    );
    assert_eq!(
        normalize(&table_sql(connection, "capacity_reservation_bound_storage_paths").await?),
        normalize(ATTACHMENT_PATHS)
    );
    assert_eq!(foreign_key_errors(connection).await?, 0);
    assert_eq!(
        count_named(connection, "index", "capacity_bound_attachment_mount").await?,
        1
    );
    for trigger in [
        "root_backing_insert_guard",
        "root_backing_identity_immutable",
        "root_backing_no_delete",
        "root_backing_bound_guard",
        "bound_attachment_no_update",
        "bound_attachment_no_delete",
        "bound_storage_path_insert_guard",
        "bound_storage_path_no_update",
        "bound_storage_path_no_delete",
    ] {
        assert_eq!(
            count_named(connection, "trigger", trigger).await?,
            1,
            "The migrated trigger must exist."
        );
    }
    assert!(connection
        .execute(
            "UPDATE capacity_reservation_provisioning_state SET resources_bound = 1 WHERE reservation_id = 1",
            (),
        )
        .await
        .is_err());
    assert_eq!(
        count(connection, "capacity_reservation_provisioning_state").await?,
        1
    );
    Ok(())
}

async fn assert_dind_path_contract(connection: &turso::Connection) -> Result<(), HostError> {
    insert_attachment(
        connection,
        "engine-metadata",
        "dind",
        "/.velnor-runtime-identity",
    )
    .await?;
    insert_path(
        connection,
        "engine-metadata",
        "dind",
        "/.velnor-runtime-identity",
        "/opt/velnor/init/dind/runtime-identity",
        Some("runtime-identity"),
    )
    .await?;
    assert!(
        insert_attachment(
            connection,
            "engine-metadata",
            "dind",
            "/opt/velnor/source-proof/control"
        )
        .await
        .is_err()
    );
    assert!(
        insert_path(
            connection,
            "engine-metadata",
            "dind",
            "/opt/velnor/job-facts",
            "/opt/velnor/init/dind/job-facts",
            Some("wrong-subpath")
        )
        .await
        .is_err()
    );
    Ok(())
}

async fn insert_attachment(
    connection: &turso::Connection,
    role: &str,
    observer: &str,
    target: &str,
) -> Result<(), HostError> {
    let container = (observer != "probe").then_some(format!("{observer}-container"));
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_attachments VALUES (1, ?1, ?2, ?3, ?4, NULL, 1, 30, 0, 38, 0, 38, 1, NULL, NULL, '0123456789abcdef', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')",
            turso::params![role, observer, container, target],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn insert_path(
    connection: &turso::Connection,
    role: &str,
    observer: &str,
    target: &str,
    root: &str,
    subpath: Option<&str>,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_paths VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            turso::params![role, observer, target, root, subpath],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn table_sql(connection: &turso::Connection, name: &str) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [name],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)
}

async fn count(connection: &turso::Connection, table: &str) -> Result<i64, HostError> {
    let mut rows = connection
        .query(&format!("SELECT COUNT(*) FROM {table}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)
}

async fn count_named(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = ?1 AND name = ?2",
            (kind, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)
}

async fn foreign_key_errors(connection: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = connection
        .query("PRAGMA foreign_key_check", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut errors = 0;
    while rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        errors += 1;
    }
    Ok(errors)
}

fn normalize(sql: &str) -> String {
    crate::journal::schema::sql_normalization::normalize(sql)
}

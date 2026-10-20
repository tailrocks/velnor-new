use crate::error::HostError;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::{ATTACHMENT_PATHS, TABLE, TRIGGERS};
use super::{PATHS, attachment_paths_with_inode, upgrade_v31};

#[tokio::test]
async fn v30_path_migration_keeps_old_inode_unknown_and_rejects_new_unknown()
-> Result<(), HostError> {
    let serial = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("velnor-v31-{}-{serial}.db", std::process::id()));
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    create_fixture(&connection).await?;
    connection
        .execute(ATTACHMENT_PATHS, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection.execute(
        "INSERT INTO capacity_reservation_bound_storage_attachments VALUES (1, 'engine-metadata', 'runner', '/.velnor-runtime-identity')",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    connection.execute(
        "INSERT INTO capacity_reservation_bound_storage_paths VALUES (1, 'engine-metadata', 'runner', '/.velnor-runtime-identity', '/metadata/runtime-identity', 'runtime-identity')",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    for (_, trigger) in TRIGGERS {
        connection
            .execute(trigger, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }

    upgrade_v31(&connection).await?;

    let mut rows = connection.query(
        "SELECT statx_inode FROM capacity_reservation_bound_storage_paths WHERE reservation_id = 1",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    let inode = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<Option<i64>>(0)
        .map_err(|_| HostError::Journal)?;
    assert_eq!(inode, None);
    connection.execute(
        "INSERT INTO capacity_reservation_provisioning_state VALUES (2, 'started', 0, 'runner-container', 'dind-container')",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    connection.execute(
        "INSERT INTO capacity_reservation_bound_storage_attachments VALUES (2, 'engine-metadata', 'runner', '/opt/velnor/job-facts')",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    connection.execute(
        "INSERT INTO capacity_reservation_bound_storage_paths (reservation_id, role, observer, mount_target, mount_root, statx_inode, volume_subpath) VALUES (2, 'engine-metadata', 'runner', '/opt/velnor/job-facts', '/metadata/job-facts', 55, 'job-facts')",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    connection.execute(
        "INSERT INTO capacity_reservation_provisioning_state VALUES (3, 'started', 0, 'runner-container', 'dind-container')",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    connection.execute(
        "INSERT INTO capacity_reservation_bound_storage_attachments VALUES (3, 'engine-metadata', 'runner', '/opt/velnor/job-facts')",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    assert!(connection.execute(
        "INSERT INTO capacity_reservation_bound_storage_paths (reservation_id, role, observer, mount_target, mount_root, statx_inode, volume_subpath) VALUES (3, 'engine-metadata', 'runner', '/opt/velnor/job-facts', '/metadata/job-facts', NULL, 'job-facts')",
        (),
    ).await.is_err());
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [PATHS],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let schema = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    assert!(
        crate::journal::schema::sql_normalization::normalize(&schema)
            == crate::journal::schema::sql_normalization::normalize(&attachment_paths_with_inode())
    );
    Ok(())
}

#[tokio::test]
async fn v30_path_migration_rejects_a_drifted_root_guard() -> Result<(), HostError> {
    let serial = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "velnor-v31-guard-{}-{serial}.db",
        std::process::id()
    ));
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    create_fixture(&connection).await?;
    connection
        .execute(ATTACHMENT_PATHS, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection.execute(
        "INSERT INTO capacity_reservation_bound_storage_attachments VALUES (1, 'engine-metadata', 'runner', '/.velnor-runtime-identity')",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    connection.execute(
        "INSERT INTO capacity_reservation_bound_storage_paths VALUES (1, 'engine-metadata', 'runner', '/.velnor-runtime-identity', '/metadata/runtime-identity', 'runtime-identity')",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    for (_, trigger) in TRIGGERS {
        connection
            .execute(trigger, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute("DROP TRIGGER root_backing_bound_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TRIGGER root_backing_bound_guard BEFORE UPDATE ON capacity_reservation_provisioning_state BEGIN SELECT 1; END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;

    assert!(upgrade_v31(&connection).await.is_err());
    assert_eq!(count_paths(&connection).await?, 1);
    connection
        .execute("DROP TRIGGER root_backing_bound_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(upgrade_v31(&connection).await.is_err());
    assert_eq!(count_paths(&connection).await?, 1);
    let schema = table_sql(&connection).await?;
    assert_eq!(
        crate::journal::schema::sql_normalization::normalize(&schema),
        crate::journal::schema::sql_normalization::normalize(ATTACHMENT_PATHS)
    );
    Ok(())
}

async fn count_paths(connection: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_bound_storage_paths",
            (),
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

async fn table_sql(connection: &turso::Connection) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'capacity_reservation_bound_storage_paths'",
            (),
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

async fn create_fixture(connection: &turso::Connection) -> Result<(), HostError> {
    for ddl in [
        "CREATE TABLE capacity_reservations (id INTEGER PRIMARY KEY, connection_id TEXT, intent_id INTEGER)",
        "CREATE TABLE connection_intents (id INTEGER PRIMARY KEY, connection_id TEXT, state TEXT, cleanup_proven INTEGER)",
        "CREATE TABLE capacity_reservation_provisioning (reservation_id INTEGER PRIMARY KEY, operation_id INTEGER, engine_id TEXT, docker_root_digest TEXT, sample_generation_id TEXT)",
        "CREATE TABLE capacity_reservation_provisioning_state (reservation_id INTEGER PRIMARY KEY, bootstrap_start_state TEXT, resources_bound INTEGER, runner_container_id TEXT, dind_container_id TEXT)",
        "CREATE TABLE capacity_reservation_bootstrap_containers (reservation_id INTEGER, role TEXT, create_state TEXT, start_state TEXT, container_id TEXT)",
        "CREATE TABLE capacity_reservation_bound_storage_attachments (reservation_id INTEGER, role TEXT, observer TEXT, mount_target TEXT, PRIMARY KEY (reservation_id, role, observer, mount_target))",
    ] {
        connection
            .execute(ddl, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(TABLE, ())
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
            "INSERT INTO connection_intents VALUES (1, 'connection', 'pending', 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection.execute(
        "INSERT INTO capacity_reservation_provisioning VALUES (1, 1, 'engine', 'root', 'sample')",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    connection.execute(
        "INSERT INTO capacity_reservation_provisioning_state VALUES (1, 'started', 0, 'runner-container', 'dind-container')",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    Ok(())
}

use crate::error::HostError;

use super::{ATTACHMENT_PATHS, PATH_INODE_TRIGGER, TRIGGERS, attachment_paths_with_inode};

const PATHS: &str = "capacity_reservation_bound_storage_paths";
const REPLACEMENT: &str = "capacity_reservation_bound_storage_paths_v31";

pub(in crate::journal::schema) async fn upgrade_v31(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let current = table_sql(connection, PATHS).await?;
    let target = attachment_paths_with_inode();
    if normalize(&current) == normalize(&target) {
        return Ok(());
    }
    if normalize(&current) != normalize(ATTACHMENT_PATHS) {
        return Err(HostError::Journal);
    }
    validate_path_triggers(connection).await?;
    rebuild_paths(connection, &target).await?;
    verify_paths(connection).await
}

async fn validate_path_triggers(connection: &turso::Connection) -> Result<(), HostError> {
    for (name, expected) in TRIGGERS.iter().filter(|(name, _)| {
        name.starts_with("bound_storage_path_") || *name == "root_backing_bound_guard"
    }) {
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
                [*name],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let sql = row.get::<String>(0).map_err(|_| HostError::Journal)?;
        if normalize(&sql) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn rebuild_paths(connection: &turso::Connection, target: &str) -> Result<(), HostError> {
    drop_path_triggers(connection).await?;
    let replacement = target.replace(PATHS, REPLACEMENT);
    connection
        .execute(&replacement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_paths_v31 (reservation_id, role, observer, mount_target, mount_root, statx_inode, volume_subpath) SELECT reservation_id, role, observer, mount_target, mount_root, NULL, volume_subpath FROM capacity_reservation_bound_storage_paths",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&format!("DROP TABLE {PATHS}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&format!("ALTER TABLE {REPLACEMENT} RENAME TO {PATHS}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    restore_triggers(connection).await
}

async fn drop_path_triggers(connection: &turso::Connection) -> Result<(), HostError> {
    for name in [
        "root_backing_bound_guard",
        "bound_storage_path_insert_guard",
        "bound_storage_path_no_update",
        "bound_storage_path_no_delete",
    ] {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn restore_triggers(connection: &turso::Connection) -> Result<(), HostError> {
    for (_, sql) in TRIGGERS {
        connection
            .execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(PATH_INODE_TRIGGER.1, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn verify_paths(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(&format!("SELECT COUNT(*) FROM {PATHS}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    let mut rows = connection
        .query("PRAGMA foreign_key_check", ())
        .await
        .map_err(|_| HostError::Journal)?;
    if count < 0 || rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn table_sql(connection: &turso::Connection, table: &str) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [table],
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

fn normalize(value: &str) -> String {
    crate::journal::schema::sql_normalization::normalize(value)
}

#[cfg(test)]
#[path = "migration_v31_tests.rs"]
mod tests;

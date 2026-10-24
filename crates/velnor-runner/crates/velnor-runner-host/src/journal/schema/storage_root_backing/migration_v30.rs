use crate::error::HostError;

mod rebuild;

use super::{ATTACHMENT_PATHS, V29_ATTACHMENT_PATHS, attachment_paths_with_inode};
use crate::journal::schema::provisioning::{
    BOUND_STORAGE_ATTACHMENTS, V29_BOUND_STORAGE_ATTACHMENTS,
};

const ATTACHMENTS: &str = "capacity_reservation_bound_storage_attachments";
const PATHS: &str = "capacity_reservation_bound_storage_paths";

pub(in crate::journal::schema) async fn upgrade_v30(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let attachment_sql = table_sql(connection, ATTACHMENTS).await?;
    let path_sql = table_sql(connection, PATHS).await?;
    let current = normalize(&attachment_sql) == normalize(BOUND_STORAGE_ATTACHMENTS)
        && (normalize(&path_sql) == normalize(ATTACHMENT_PATHS)
            || normalize(&path_sql) == normalize(&attachment_paths_with_inode()));
    if current {
        return Ok(());
    }
    if normalize(&attachment_sql) != normalize(V29_BOUND_STORAGE_ATTACHMENTS)
        || normalize(&path_sql) != normalize(V29_ATTACHMENT_PATHS)
    {
        return Err(HostError::Journal);
    }

    let attachment_count = count(connection, ATTACHMENTS).await?;
    let path_count = count(connection, PATHS).await?;
    drop_guards(connection).await?;
    rebuild::rebuild_v29_tables(connection, attachment_count, path_count).await?;
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

pub(super) async fn foreign_key_errors(connection: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = connection
        .query("PRAGMA foreign_key_check", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut count = 0;
    while rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        count += 1;
    }
    Ok(count)
}

async fn drop_guards(connection: &turso::Connection) -> Result<(), HostError> {
    for trigger in [
        "bound_attachment_no_update",
        "bound_attachment_no_delete",
        "root_backing_bound_guard",
        "bound_storage_path_insert_guard",
        "bound_storage_path_no_update",
        "bound_storage_path_no_delete",
    ] {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {trigger}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute("DROP INDEX IF EXISTS capacity_bound_attachment_mount", ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)
}

fn normalize(sql: &str) -> String {
    crate::journal::schema::sql_normalization::normalize(sql)
}

#[cfg(test)]
#[path = "migration_v30_tests.rs"]
mod tests;

use crate::error::HostError;

use super::{
    ATTACHMENT_PATHS, LEGACY_ATTACHMENT_PATHS, PATH_INODE_TRIGGER, TRIGGERS, V29_ATTACHMENT_PATHS,
    attachment_paths_with_inode, migration_v30, normalize,
};
use crate::journal::schema::provisioning::{
    BOUND_STORAGE_ATTACHMENTS, LEGACY_BOUND_STORAGE_ATTACHMENTS, V29_BOUND_STORAGE_ATTACHMENTS,
};

const ATTACHMENTS: &str = "capacity_reservation_bound_storage_attachments";
const PATHS: &str = "capacity_reservation_bound_storage_paths";

pub(in crate::journal::schema) async fn upgrade_v27(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let attachment_sql = table_sql(connection, ATTACHMENTS).await?;
    let path_sql = table_sql(connection, PATHS).await?;
    if is_current_v31_pair(attachment_sql.as_deref(), path_sql.as_deref()) {
        validate_current_v31_guards(connection).await?;
        return Ok(());
    }
    let Some(attachment_sql) = attachment_sql else {
        return absent_attachment(path_sql);
    };
    if is_v29_pair(&attachment_sql, path_sql.as_deref()) {
        return migration_v30::upgrade_v30(connection).await;
    }
    if is_v29_with_legacy_paths(&attachment_sql, path_sql.as_deref()) {
        upgrade_legacy_paths_to_v29(connection).await?;
        return migration_v30::upgrade_v30(connection).await;
    }
    let (rebuild_attachments, rebuild_paths) = rebuild_plan(&attachment_sql, path_sql.as_deref())?;
    if rebuild_attachments || rebuild_paths {
        rebuild_supported_schema(connection, rebuild_attachments, rebuild_paths).await?;
    }
    Ok(())
}

fn is_current_v31_pair(attachment_sql: Option<&str>, path_sql: Option<&str>) -> bool {
    attachment_sql.is_some_and(|sql| normalize(sql) == normalize(BOUND_STORAGE_ATTACHMENTS))
        && path_sql.is_some_and(|sql| normalize(sql) == normalize(&attachment_paths_with_inode()))
}

async fn validate_current_v31_guards(connection: &turso::Connection) -> Result<(), HostError> {
    for (name, expected) in TRIGGERS
        .iter()
        .filter(|(name, _)| {
            name.starts_with("bound_storage_path_") || *name == "root_backing_bound_guard"
        })
        .chain(std::iter::once(&PATH_INODE_TRIGGER))
    {
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

fn absent_attachment(path_sql: Option<String>) -> Result<(), HostError> {
    if path_sql.is_some() {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

fn is_v29_pair(attachment_sql: &str, path_sql: Option<&str>) -> bool {
    normalize(attachment_sql) == normalize(V29_BOUND_STORAGE_ATTACHMENTS)
        && path_sql.is_some_and(|sql| normalize(sql) == normalize(V29_ATTACHMENT_PATHS))
}

fn is_v29_with_legacy_paths(attachment_sql: &str, path_sql: Option<&str>) -> bool {
    normalize(attachment_sql) == normalize(V29_BOUND_STORAGE_ATTACHMENTS)
        && path_sql.is_some_and(|sql| normalize(sql) == normalize(LEGACY_ATTACHMENT_PATHS))
}

fn rebuild_plan(attachment_sql: &str, path_sql: Option<&str>) -> Result<(bool, bool), HostError> {
    let rebuild_attachments =
        normalize(attachment_sql) == normalize(LEGACY_BOUND_STORAGE_ATTACHMENTS);
    let attachment_supported =
        rebuild_attachments || normalize(attachment_sql) == normalize(BOUND_STORAGE_ATTACHMENTS);
    if !attachment_supported {
        return Err(HostError::Journal);
    }
    let path_current = path_sql.is_some_and(|sql| normalize(sql) == normalize(ATTACHMENT_PATHS));
    let path_legacy =
        path_sql.is_some_and(|sql| normalize(sql) == normalize(LEGACY_ATTACHMENT_PATHS));
    if path_sql.is_some() && !path_current && !path_legacy {
        return Err(HostError::Journal);
    }
    Ok((
        rebuild_attachments,
        path_sql.is_some() && (!path_current || rebuild_attachments),
    ))
}

async fn rebuild_supported_schema(
    connection: &turso::Connection,
    rebuild_attachments: bool,
    rebuild_paths: bool,
) -> Result<(), HostError> {
    let attachment_count = if rebuild_attachments {
        table_count(connection, ATTACHMENTS).await?
    } else {
        0
    };
    let path_count = if rebuild_paths {
        table_count(connection, PATHS).await?
    } else {
        0
    };
    drop_rebuild_guards(connection, rebuild_attachments, rebuild_paths).await?;
    create_replacements(connection, rebuild_attachments, rebuild_paths).await?;
    remove_old_tables(connection, rebuild_attachments, rebuild_paths).await?;
    restore_replacement_guards(connection, rebuild_attachments, rebuild_paths).await?;
    verify_rebuilt_counts(
        connection,
        attachment_count,
        path_count,
        rebuild_attachments,
        rebuild_paths,
    )
    .await
}

async fn drop_rebuild_guards(
    connection: &turso::Connection,
    rebuild_attachments: bool,
    rebuild_paths: bool,
) -> Result<(), HostError> {
    if rebuild_paths {
        for trigger in [
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
    }
    if rebuild_attachments {
        for object in [
            "DROP TRIGGER IF EXISTS bound_attachment_no_update",
            "DROP TRIGGER IF EXISTS bound_attachment_no_delete",
            "DROP INDEX IF EXISTS capacity_bound_attachment_mount",
        ] {
            connection
                .execute(object, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}

async fn create_replacements(
    connection: &turso::Connection,
    rebuild_attachments: bool,
    rebuild_paths: bool,
) -> Result<(), HostError> {
    if rebuild_attachments {
        create_attachment_replacement(connection).await?;
    }
    if rebuild_paths {
        create_path_replacement(connection, rebuild_attachments).await?;
    }
    Ok(())
}

async fn create_attachment_replacement(connection: &turso::Connection) -> Result<(), HostError> {
    let replacement = BOUND_STORAGE_ATTACHMENTS.replace(
        ATTACHMENTS,
        "capacity_reservation_bound_storage_attachments_v27",
    );
    connection
        .execute(&replacement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_attachments_v27 (reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest) SELECT reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest FROM capacity_reservation_bound_storage_attachments",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn create_path_replacement(
    connection: &turso::Connection,
    rebuild_attachments: bool,
) -> Result<(), HostError> {
    let mut replacement =
        ATTACHMENT_PATHS.replace(PATHS, "capacity_reservation_bound_storage_paths_v27");
    if rebuild_attachments {
        replacement = replacement.replace(
            "REFERENCES capacity_reservation_bound_storage_attachments(",
            "REFERENCES capacity_reservation_bound_storage_attachments_v27(",
        );
    }
    connection
        .execute(&replacement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_paths_v27 (reservation_id, role, observer, mount_target, mount_root, volume_subpath) SELECT reservation_id, role, observer, mount_target, mount_root, volume_subpath FROM capacity_reservation_bound_storage_paths",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn remove_old_tables(
    connection: &turso::Connection,
    rebuild_attachments: bool,
    rebuild_paths: bool,
) -> Result<(), HostError> {
    if rebuild_paths {
        connection
            .execute(&format!("DROP TABLE {PATHS}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    if rebuild_attachments {
        connection
            .execute(&format!("DROP TABLE {ATTACHMENTS}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "ALTER TABLE capacity_reservation_bound_storage_attachments_v27 RENAME TO capacity_reservation_bound_storage_attachments",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        super::super::provisioning::restore_bound_storage_attachment_guards(connection).await?;
    }
    if rebuild_paths {
        connection
            .execute(
                "ALTER TABLE capacity_reservation_bound_storage_paths_v27 RENAME TO capacity_reservation_bound_storage_paths",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn restore_replacement_guards(
    connection: &turso::Connection,
    _rebuild_attachments: bool,
    rebuild_paths: bool,
) -> Result<(), HostError> {
    if rebuild_paths {
        for (_, sql) in TRIGGERS.iter().skip(3) {
            connection
                .execute(sql, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}

async fn verify_rebuilt_counts(
    connection: &turso::Connection,
    attachment_count: i64,
    path_count: i64,
    rebuild_attachments: bool,
    rebuild_paths: bool,
) -> Result<(), HostError> {
    if rebuild_attachments && table_count(connection, ATTACHMENTS).await? != attachment_count {
        return Err(HostError::Journal);
    }
    if rebuild_paths && table_count(connection, PATHS).await? != path_count {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn upgrade_legacy_paths_to_v29(connection: &turso::Connection) -> Result<(), HostError> {
    let original_count = table_count(connection, PATHS).await?;
    drop_legacy_path_guards(connection).await?;
    copy_legacy_path_table(connection).await?;
    restore_legacy_path_guards(connection).await?;
    if table_count(connection, PATHS).await? != original_count
        || migration_v30::foreign_key_errors(connection).await? != 0
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn drop_legacy_path_guards(connection: &turso::Connection) -> Result<(), HostError> {
    for trigger in [
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
    Ok(())
}

async fn copy_legacy_path_table(connection: &turso::Connection) -> Result<(), HostError> {
    let replacement =
        V29_ATTACHMENT_PATHS.replace(PATHS, "capacity_reservation_bound_storage_paths_v27");
    connection
        .execute(&replacement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_paths_v27 (reservation_id, role, observer, mount_target, mount_root, volume_subpath) SELECT reservation_id, role, observer, mount_target, mount_root, volume_subpath FROM capacity_reservation_bound_storage_paths",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&format!("DROP TABLE {PATHS}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "ALTER TABLE capacity_reservation_bound_storage_paths_v27 RENAME TO capacity_reservation_bound_storage_paths",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn restore_legacy_path_guards(connection: &turso::Connection) -> Result<(), HostError> {
    for (_, trigger) in TRIGGERS.iter().skip(3) {
        connection
            .execute(trigger, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn table_sql(
    connection: &turso::Connection,
    name: &str,
) -> Result<Option<String>, HostError> {
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
        .map(|row| row.get::<String>(0).map_err(|_| HostError::Journal))
        .transpose()
}

async fn table_count(connection: &turso::Connection, table: &str) -> Result<i64, HostError> {
    scalar_count(connection, &format!("SELECT COUNT(*) FROM {table}")).await
}

async fn scalar_count(connection: &turso::Connection, sql: &str) -> Result<i64, HostError> {
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

use crate::error::HostError;

use super::{ATTACHMENTS, PATHS};
use crate::journal::schema::provisioning::{
    BOUND_STORAGE_ATTACHMENTS, restore_bound_storage_attachment_guards,
};
use crate::journal::schema::storage_root_backing::{ATTACHMENT_PATHS, TRIGGERS};

pub(super) async fn rebuild_v29_tables(
    connection: &turso::Connection,
    attachment_count: i64,
    path_count: i64,
) -> Result<(), HostError> {
    create_v30_tables(connection).await?;
    drop_v29_tables(connection).await?;
    restore_v30_guards(connection).await?;
    verify_v30_tables(connection, attachment_count, path_count).await
}

async fn create_v30_tables(connection: &turso::Connection) -> Result<(), HostError> {
    let attachment_replacement = BOUND_STORAGE_ATTACHMENTS.replace(
        ATTACHMENTS,
        "capacity_reservation_bound_storage_attachments_v30",
    );
    connection
        .execute(&attachment_replacement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    copy_attachments(connection).await?;
    create_path_table(connection).await?;
    copy_paths(connection).await
}

async fn copy_attachments(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_attachments_v30 (reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest) SELECT reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest FROM capacity_reservation_bound_storage_attachments",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn create_path_table(connection: &turso::Connection) -> Result<(), HostError> {
    let replacement = ATTACHMENT_PATHS
        .replace(PATHS, "capacity_reservation_bound_storage_paths_v30")
        .replace(
            "REFERENCES capacity_reservation_bound_storage_attachments(",
            "REFERENCES capacity_reservation_bound_storage_attachments_v30(",
        );
    connection
        .execute(&replacement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn copy_paths(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_paths_v30 (reservation_id, role, observer, mount_target, mount_root, volume_subpath) SELECT reservation_id, role, observer, mount_target, mount_root, volume_subpath FROM capacity_reservation_bound_storage_paths",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn drop_v29_tables(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(&format!("DROP TABLE {PATHS}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&format!("DROP TABLE {ATTACHMENTS}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "ALTER TABLE capacity_reservation_bound_storage_attachments_v30 RENAME TO capacity_reservation_bound_storage_attachments",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    restore_bound_storage_attachment_guards(connection).await?;
    connection
        .execute(
            "ALTER TABLE capacity_reservation_bound_storage_paths_v30 RENAME TO capacity_reservation_bound_storage_paths",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn restore_v30_guards(connection: &turso::Connection) -> Result<(), HostError> {
    for (_, trigger) in TRIGGERS.iter().skip(3) {
        connection
            .execute(trigger, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn verify_v30_tables(
    connection: &turso::Connection,
    attachment_count: i64,
    path_count: i64,
) -> Result<(), HostError> {
    if super::count(connection, ATTACHMENTS).await? != attachment_count
        || super::count(connection, PATHS).await? != path_count
        || super::foreign_key_errors(connection).await? != 0
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

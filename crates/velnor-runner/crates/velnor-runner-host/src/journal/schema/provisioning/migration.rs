/// Expand v16's two-word filesystem IDs without guessing their original encoding.
pub(super) async fn upgrade_from_version_sixteen(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    upgrade_attachments(connection).await?;
    upgrade_publication(connection).await?;
    bootstrap(connection).await?;
    connection
        .execute(
            "UPDATE capacity_reservations SET state = 'uncertain' WHERE state = 'released' AND EXISTS (SELECT 1 FROM capacity_reservation_provisioning_state AS state WHERE state.reservation_id = capacity_reservations.id AND state.resources_bound = 1 AND state.storage_cleanup_proven = 1)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

#[cfg(test)]
pub(in crate::journal) async fn upgrade_v16_filesystem_ids_for_test(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    upgrade_attachments(connection).await?;
    upgrade_publication(connection).await
}

async fn upgrade_attachments(connection: &turso::Connection) -> Result<(), HostError> {
    let attachment_table = "capacity_reservation_bound_storage_attachments";
    let attachment_v16 = [
        "reservation_id",
        "role",
        "observer",
        "container_id",
        "mount_target",
        "source_target",
        "read_only",
        "mount_id",
        "device_major",
        "device_minor",
        "statx_device_major",
        "statx_device_minor",
        "fs_type_magic",
        "filesystem_id_0",
        "filesystem_id_1",
        "filesystem_instance_digest",
    ];
    let attachment_v17 = [
        "reservation_id",
        "role",
        "observer",
        "container_id",
        "mount_target",
        "source_target",
        "read_only",
        "mount_id",
        "device_major",
        "device_minor",
        "statx_device_major",
        "statx_device_minor",
        "fs_type_magic",
        "v16_filesystem_id_0",
        "v16_filesystem_id_1",
        "filesystem_id",
        "filesystem_instance_digest",
    ];
    let attachment_columns = table_columns(connection, attachment_table).await?;
    if attachment_columns
        .iter()
        .map(String::as_str)
        .eq(attachment_v16)
    {
        rebuild_v16_filesystem_table(
            connection,
            attachment_table,
            &attachment_v16,
            "bound_attachment_no_update",
            "bound_attachment_no_delete",
            Some("capacity_bound_attachment_mount"),
            "INSERT INTO capacity_reservation_bound_storage_attachments (reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest) SELECT reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, filesystem_id_0, filesystem_id_1, NULL, filesystem_instance_digest FROM capacity_reservation_bound_storage_attachments_v16",
        )
        .await?;
    } else if !attachment_columns
        .iter()
        .map(String::as_str)
        .eq(attachment_v17)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn upgrade_publication(connection: &turso::Connection) -> Result<(), HostError> {
    let publication_table = "capacity_reservation_provisioning_publications";
    let publication_v16 = [
        "reservation_id",
        "runner_id",
        "runner_name",
        "runner_scale_set_id",
        "target",
        "projection_sha256",
        "directory_device_major",
        "directory_device_minor",
        "directory_inode",
        "mount_id",
        "filesystem_id_0",
        "filesystem_id_1",
        "filesystem_instance_digest",
        "read_only",
    ];
    let publication_v17 = [
        "reservation_id",
        "runner_id",
        "runner_name",
        "runner_scale_set_id",
        "target",
        "projection_sha256",
        "directory_device_major",
        "directory_device_minor",
        "directory_inode",
        "mount_id",
        "v16_filesystem_id_0",
        "v16_filesystem_id_1",
        "filesystem_id",
        "filesystem_instance_digest",
        "read_only",
    ];
    let publication_columns = table_columns(connection, publication_table).await?;
    if publication_columns
        .iter()
        .map(String::as_str)
        .eq(publication_v16)
    {
        rebuild_v16_filesystem_table(
            connection,
            publication_table,
            &publication_v16,
            "provisioning_publication_no_update",
            "provisioning_publication_no_delete",
            None,
            "INSERT INTO capacity_reservation_provisioning_publications (reservation_id, runner_id, runner_name, runner_scale_set_id, target, projection_sha256, directory_device_major, directory_device_minor, directory_inode, mount_id, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest, read_only) SELECT reservation_id, runner_id, runner_name, runner_scale_set_id, target, projection_sha256, directory_device_major, directory_device_minor, directory_inode, mount_id, filesystem_id_0, filesystem_id_1, NULL, filesystem_instance_digest, read_only FROM capacity_reservation_provisioning_publications_v16",
        )
        .await?;
    } else if !publication_columns
        .iter()
        .map(String::as_str)
        .eq(publication_v17)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}
async fn rebuild_v16_filesystem_table(
    connection: &turso::Connection,
    table: &str,
    expected_columns: &[&str],
    immutable_update_trigger: &str,
    immutable_delete_trigger: &str,
    index: Option<&str>,
    copy_statement: &str,
) -> Result<(), HostError> {
    validate_v16_columns(connection, table, expected_columns).await?;
    for trigger in [immutable_update_trigger, immutable_delete_trigger] {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {trigger}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    if let Some(index) = index {
        connection
            .execute(&format!("DROP INDEX IF EXISTS {index}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    let legacy = format!("{table}_v16");
    connection
        .execute(&format!("ALTER TABLE {table} RENAME TO {legacy}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let statement = TABLES
        .iter()
        .find(|(name, _)| *name == table)
        .map(|(_, statement)| *statement)
        .ok_or(HostError::Journal)?;
    connection
        .execute(statement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(copy_statement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&format!("DROP TABLE {legacy}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn validate_v16_columns(
    connection: &turso::Connection,
    table: &str,
    expected: &[&str],
) -> Result<(), HostError> {
    let actual = table_columns(connection, table).await?;
    if actual
        .iter()
        .map(String::as_str)
        .ne(expected.iter().copied())
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn table_columns(
    connection: &turso::Connection,
    table: &str,
) -> Result<Vec<String>, HostError> {
    let mut rows = connection
        .query(&format!("PRAGMA table_info({table})"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut actual = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        actual.push(row.get::<String>(1).map_err(|_| HostError::Journal)?);
    }
    Ok(actual)
}

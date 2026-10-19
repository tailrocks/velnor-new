//! Execution publication schema migrations.

use crate::error::HostError;

use super::{
    CONTEXT_DELETE_GUARD, CONTEXT_INSERT_GUARD, CONTEXT_TABLE, CONTEXT_UPDATE_GUARD,
    PARENT_CLEANUP_GUARD, PARENT_RELEASE_GUARD, validate,
};

/// Add nullable recovery facts without inventing facts for earlier operations.
pub(in crate::journal::schema) async fn upgrade_v25(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    ensure_context_column(
        connection,
        "recovery_projection_json",
        "recovery_projection_json TEXT CHECK (recovery_projection_json IS NULL OR length(CAST(recovery_projection_json AS BLOB)) <= 32768)",
    )
    .await?;
    ensure_context_column(
        connection,
        "recovery_projection_sha256",
        "recovery_projection_sha256 TEXT CHECK (recovery_projection_sha256 IS NULL OR (length(recovery_projection_sha256) = 64 AND recovery_projection_sha256 NOT GLOB '*[^0-9a-f]*'))",
    )
    .await?;
    validate_v25(connection).await
}

/// Check that every recovery projection is bounded and has a matching digest.
pub(in crate::journal::schema) async fn validate_v25(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    validate_context_columns(connection).await?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM execution_publication_operations WHERE (recovery_projection_json IS NULL) != (recovery_projection_sha256 IS NULL) OR length(CAST(recovery_projection_json AS BLOB)) > 32768 OR (recovery_projection_sha256 IS NOT NULL AND (length(recovery_projection_sha256) != 64 OR recovery_projection_sha256 GLOB '*[^0-9a-f]*'))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if count == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

/// Add the cache-admission parent kind and keep prior publication rows.
pub(in crate::journal::schema) async fn upgrade_v27(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    rebuild_v27(connection).await
}

pub(super) async fn rebuild_v27(connection: &turso::Connection) -> Result<(), HostError> {
    for name in [
        "execution_publication_context_insert_guard",
        "execution_publication_context_update_guard",
        "execution_publication_context_delete_guard",
        "execution_publication_parent_cleanup_guard",
        "execution_publication_parent_release_guard",
    ] {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(
            "ALTER TABLE execution_publication_operations RENAME TO execution_publication_operations_v26",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(CONTEXT_TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO execution_publication_operations (operation_id, parent_kind, parent_operation_id, reservation_id, connection_id, scale_set_id, witness_request_id, actual_request_id, runner_id, runner_name, runner_container_id, dind_container_id, engine_id, docker_root_digest, metadata_volume_name, metadata_volume_id, metadata_layout_version, metadata_layout_digest, guest_boot_id, payload_sha256, claim_digest, projection_digest, context_digest, recovery_projection_json, recovery_projection_sha256) SELECT operation_id, parent_kind, parent_operation_id, reservation_id, connection_id, scale_set_id, witness_request_id, actual_request_id, runner_id, runner_name, runner_container_id, dind_container_id, engine_id, docker_root_digest, metadata_volume_name, metadata_volume_id, metadata_layout_version, metadata_layout_digest, guest_boot_id, payload_sha256, claim_digest, projection_digest, context_digest, recovery_projection_json, recovery_projection_sha256 FROM execution_publication_operations_v26",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE execution_publication_operations_v26", ())
        .await
        .map_err(|_| HostError::Journal)?;
    for trigger in [
        CONTEXT_INSERT_GUARD,
        CONTEXT_UPDATE_GUARD,
        CONTEXT_DELETE_GUARD,
        PARENT_CLEANUP_GUARD,
        PARENT_RELEASE_GUARD,
    ] {
        connection
            .execute(trigger, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    validate_v27(connection).await
}

/// Check the parent kinds and guards after migration.
pub(in crate::journal::schema) async fn validate_v27(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    validate(connection).await?;
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'execution_publication_operations'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let sql = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    if !sql.contains("'job-facts'")
        || !sql.contains("witness_request_id IS NULL")
        || !sql.contains("actual_request_id IS NULL")
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn ensure_column(
    connection: &turso::Connection,
    name: &str,
    definition: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query("PRAGMA table_info(resource_probe_operations)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        if row.get::<String>(1).map_err(|_| HostError::Journal)? == name {
            return Ok(());
        }
    }
    connection
        .execute(
            &format!("ALTER TABLE resource_probe_operations ADD COLUMN {definition}"),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn ensure_context_column(
    connection: &turso::Connection,
    name: &str,
    definition: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query("PRAGMA table_info(execution_publication_operations)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        if row.get::<String>(1).map_err(|_| HostError::Journal)? == name {
            return Ok(());
        }
    }
    connection
        .execute(
            &format!("ALTER TABLE execution_publication_operations ADD COLUMN {definition}"),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn validate_context_columns(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let mut rows = connection
        .query("PRAGMA table_info(execution_publication_operations)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut found = [false; 2];
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        if name == "recovery_projection_json" {
            found[0] = true;
        } else if name == "recovery_projection_sha256" {
            found[1] = true;
        }
    }
    if found.into_iter().all(|value| value) {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn validate_columns(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query("PRAGMA table_info(resource_probe_operations)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut found = [false; 4];
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        for (index, expected) in [
            "helper_role",
            "probe_purpose",
            "request_sha256",
            "request_context",
        ]
        .into_iter()
        .enumerate()
        {
            if name == expected {
                found[index] = true;
            }
        }
    }
    if found.into_iter().all(|present| present) {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

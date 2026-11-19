// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

use super::{ScaleSetDeleteTarget, stored_scale_set_id, stored_state};

pub(super) async fn unverified_target(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<Option<ScaleSetDeleteTarget>, HostError> {
    let mut rows = connection
        .query(
            "SELECT u.remote_id, i.selector_name, i.selector_labels, i.runner_group_id FROM connection_scale_set_unverified AS u JOIN connection_scale_set_intents AS i ON i.connection_id = u.connection_id WHERE u.connection_id = ?1 AND i.state = 'uncertain'",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let scale_set_id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
    let name = row.get::<String>(1).map_err(|_| HostError::Journal)?;
    let labels = row.get::<String>(2).map_err(|_| HostError::Journal)?;
    let runner_group_id = row.get::<i64>(3).map_err(|_| HostError::Journal)?;
    drop(rows);
    if stored_scale_set_id(connection, connection_id)
        .await?
        .is_some_and(|stored| stored != scale_set_id)
    {
        return Err(HostError::Identity);
    }
    Ok(Some(ScaleSetDeleteTarget {
        scale_set_id,
        name,
        labels: serde_json::from_str(&labels).map_err(|_| HostError::Journal)?,
        runner_group_id,
        state: stored_state(connection, connection_id).await?,
        unverified: true,
    }))
}

pub(super) async fn release_unverified(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
) -> Result<(), HostError> {
    let released = connection
        .execute(
            "DELETE FROM connection_scale_set_unverified WHERE connection_id = ?1 AND remote_id = ?2 AND EXISTS (SELECT 1 FROM connection_scale_set_intents WHERE connection_id = ?1 AND state = 'uncertain')",
            (connection_id, scale_set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if released == 0 {
        return Ok(());
    }
    for statement in [
        "UPDATE connection_scale_set_intents SET state = 'prepared' WHERE connection_id = ?1 AND state = 'uncertain'",
        "DELETE FROM connection_scale_set_deletes WHERE connection_id = ?1 AND state = 'deleted'",
    ] {
        require_one(connection.execute(statement, [connection_id]).await)?;
    }
    Ok(())
}

pub(in crate::journal::connection) async fn restart_after_deleted_set(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT d.scale_set_id FROM connection_scale_set_deletes AS d JOIN connection_scale_set_intents AS i ON i.connection_id = d.connection_id AND i.remote_id = d.scale_set_id JOIN connection_runtime_bindings AS b ON b.connection_id = d.connection_id AND b.scale_set_id = d.scale_set_id WHERE d.connection_id = ?1 AND d.state = 'deleted' AND i.state = 'created' AND b.set_ownership = 'created'",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(());
    };
    let scale_set_id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
    drop(rows);
    for statement in [
        "DELETE FROM connection_scale_set_deletes WHERE connection_id = ?1",
        "DELETE FROM connection_scale_set_intents WHERE connection_id = ?1",
    ] {
        require_one(connection.execute(statement, [connection_id]).await)?;
    }
    connection
        .execute(
            "DELETE FROM connection_scale_set_unverified WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    require_one(
        connection
            .execute(
                "UPDATE connection_runtime_bindings SET scale_set_id = NULL, scale_set_name = NULL, set_ownership = NULL WHERE connection_id = ?1 AND scale_set_id = ?2",
                (connection_id, scale_set_id),
            )
            .await,
    )
}

fn require_one(changed: Result<u64, turso::Error>) -> Result<(), HostError> {
    if changed.is_ok_and(|rows| rows == 1) {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

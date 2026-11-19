// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const OBJECTS: &[(&str, &str, &str)] = &[
    (
        "table",
        "connection_scale_set_unverified",
        "CREATE TABLE connection_scale_set_unverified (connection_id TEXT NOT NULL PRIMARY KEY CHECK (length(connection_id) > 0), remote_id INTEGER NOT NULL CHECK (remote_id > 0))",
    ),
    (
        "trigger",
        "connection_scale_set_unverified_no_update",
        "CREATE TRIGGER connection_scale_set_unverified_no_update BEFORE UPDATE ON connection_scale_set_unverified BEGIN SELECT RAISE(ABORT,'An unverified scale set id is immutable.'); END",
    ),
];

pub(super) async fn migrate_v58_to_v59(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v53(conn).await?;
    super::scale_set_delete::validate(conn).await?;
    for (_, _, sql) in OBJECTS {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    conn.execute("PRAGMA user_version=59", ())
        .await
        .map_err(|_| HostError::Journal)?;
    validate(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    for &(kind, name, expected) in OBJECTS {
        let mut rows = conn
            .query(
                "SELECT sql FROM sqlite_schema WHERE type=?1 AND name=?2",
                (kind, name),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let actual: String = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get(0)
            .map_err(|_| HostError::Journal)?;
        if super::sql_normalization::normalize(&actual)
            != super::sql_normalization::normalize(expected)
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

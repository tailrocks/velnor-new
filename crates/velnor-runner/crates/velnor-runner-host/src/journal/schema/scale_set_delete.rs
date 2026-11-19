// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const TABLE: &str = concat!(
    "CREATE TABLE connection_scale_set_deletes (",
    "id INTEGER PRIMARY KEY AUTOINCREMENT, ",
    "connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), ",
    "scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), ",
    "runner_group_id INTEGER NOT NULL CHECK (runner_group_id > 0), ",
    "state TEXT NOT NULL CHECK (state IN ('prepared', 'requested', 'uncertain', 'deleted')), ",
    "UNIQUE (connection_id))"
);

const NAME: &str = "connection_scale_set_deletes";

pub(super) async fn migrate_v57_to_v58(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v53(conn).await?;
    conn.execute(&format!("DROP TABLE IF EXISTS {NAME}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute(TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute("PRAGMA user_version = 58", ())
        .await
        .map_err(|_| HostError::Journal)?;
    validate(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    let actual = stored_sql(conn).await?.ok_or(HostError::Journal)?;
    if super::sql_normalization::normalize(&actual) == super::sql_normalization::normalize(TABLE) {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn stored_sql(conn: &turso::Connection) -> Result<Option<String>, HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [NAME],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    row.get::<String>(0)
        .map(Some)
        .map_err(|_| HostError::Journal)
}

#[cfg(test)]
#[path = "scale_set_delete/tests.rs"]
mod tests;

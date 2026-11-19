// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const OBJECTS: &[(&str, &str, &str)] = &[
    (
        "table",
        "jit_tombstone_completions",
        "CREATE TABLE jit_tombstone_completions (reservation_id INTEGER PRIMARY KEY REFERENCES jit_absent_after_deadline(reservation_id), runner_id INTEGER NOT NULL CHECK(runner_id>0))",
    ),
    (
        "trigger",
        "jit_tombstone_completions_no_update",
        "CREATE TRIGGER jit_tombstone_completions_no_update BEFORE UPDATE ON jit_tombstone_completions BEGIN SELECT RAISE(ABORT,'JIT tombstone completions are immutable.'); END",
    ),
    (
        "trigger",
        "jit_tombstone_completions_no_delete",
        "CREATE TRIGGER jit_tombstone_completions_no_delete BEFORE DELETE ON jit_tombstone_completions BEGIN SELECT RAISE(ABORT,'JIT tombstone completions are immutable.'); END",
    ),
];

pub(super) async fn migrate(conn: &turso::Connection) -> Result<(), HostError> {
    for (_, _, sql) in OBJECTS {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    conn.execute("PRAGMA user_version=50", ())
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

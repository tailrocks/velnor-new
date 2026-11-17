// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const GUARD_NAME: &str = "jit_interrupted_remote_absence_guard";
const GUARD: &str = "CREATE TRIGGER jit_interrupted_remote_absence_guard BEFORE INSERT ON physical_attempt_jit_conflict_recovery_progress WHEN (NEW.phase = 'remote_initial_absent' OR (NEW.phase = 'remote_absent' AND NEW.runner_id IS NULL)) AND EXISTS (SELECT 1 FROM physical_attempt_jit_interrupted_claims interrupted WHERE interrupted.reservation_id = NEW.reservation_id) BEGIN SELECT RAISE(ABORT, 'Interrupted JIT registration requires a remote deletion target.'); END";
const INVALID_HISTORY: &str = "SELECT EXISTS (SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress progress JOIN physical_attempt_jit_interrupted_claims interrupted ON interrupted.reservation_id = progress.reservation_id WHERE progress.phase = 'remote_initial_absent' OR (progress.phase = 'remote_absent' AND progress.runner_id IS NULL))";

pub(in crate::journal::schema) async fn migrate(conn: &turso::Connection) -> Result<(), HostError> {
    super::super::validate_persisted_schema_v46(conn).await?;
    validate_history(conn).await?;
    conn.execute(GUARD, ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute("PRAGMA user_version = 48", ())
        .await
        .map_err(|_| HostError::Journal)?;
    validate(conn).await
}

pub(in crate::journal::schema) async fn validate(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
            [GUARD_NAME],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let sql: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || super::super::sql_normalization::normalize(&sql)
            != super::super::sql_normalization::normalize(GUARD)
    {
        return Err(HostError::Journal);
    }
    validate_history(conn).await
}

async fn validate_history(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(INVALID_HISTORY, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let invalid: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if invalid != 0 {
        return Err(HostError::Journal);
    }
    Ok(())
}

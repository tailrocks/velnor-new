//! Read-only intent snapshots.

use crate::{error::HostError, reconcile::IntentRow};

use super::{Journal, intent_row};

impl Journal {
    /// Load every row. The connection closes before this returns.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on I/O or a corrupt state.
    pub async fn rows(&self) -> Result<Vec<IntentRow>, HostError> {
        let conn = self.connection().await?;
        let mut query = conn
            .query(
                "SELECT id, kind, subject, state, docker_id, github_runner_id, cleanup_proven, dind_id, worker_volume, scale_set_id, runner_request_id, runner_name, docker_engine_id, launch_phase, launch_id, assignment_key, seed_generation_id, acquire_attempted, acquire_resolved, acquired, jit_requested, runner_completed FROM intents ORDER BY id",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let mut out = Vec::new();
        while let Some(row) = query.next().await.map_err(|_| HostError::Journal)? {
            out.push(intent_row(&row)?);
        }
        Ok(out)
    }

    /// Read only active worker rows, refusing a prefix larger than the current ceiling.
    pub(crate) async fn capacity_rows(&self, ceiling: u32) -> Result<Vec<IntentRow>, HostError> {
        let limit = u64::from(ceiling)
            .checked_add(1)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(HostError::Journal)?;
        let conn = self.connection().await?;
        conn.execute("BEGIN", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let rows = capacity_rows_in_snapshot(&conn, ceiling, limit).await;
        let end = if rows.is_ok() { "COMMIT" } else { "ROLLBACK" };
        conn.execute(end, ())
            .await
            .map_err(|_| HostError::Journal)?;
        rows
    }
}

async fn capacity_rows_in_snapshot(
    conn: &turso::Connection,
    ceiling: u32,
    limit: i64,
) -> Result<Vec<IntentRow>, HostError> {
    let predicate = crate::launch::HOLDS_ROWS_SQL;
    let count_sql = format!("SELECT COUNT(*) FROM intents WHERE {predicate}");
    let mut count_rows = conn
        .query(&count_sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let count = count_rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    let count = u32::try_from(count).map_err(|_| HostError::Journal)?;
    if count > ceiling {
        return Err(HostError::Journal);
    }

    let rows_sql = format!(
        "SELECT id, kind, subject, state, docker_id, github_runner_id, cleanup_proven, dind_id, worker_volume, scale_set_id, runner_request_id, runner_name, docker_engine_id, launch_phase, launch_id, assignment_key, seed_generation_id, acquire_attempted, acquire_resolved, acquired, jit_requested, runner_completed FROM intents WHERE {predicate} ORDER BY id LIMIT ?1"
    );
    let mut query = conn
        .query(&rows_sql, [limit])
        .await
        .map_err(|_| HostError::Journal)?;
    let mut out = Vec::new();
    while let Some(row) = query.next().await.map_err(|_| HostError::Journal)? {
        out.push(intent_row(&row)?);
    }
    if out.len() != usize::try_from(count).map_err(|_| HostError::Journal)? {
        return Err(HostError::Journal);
    }
    Ok(out)
}

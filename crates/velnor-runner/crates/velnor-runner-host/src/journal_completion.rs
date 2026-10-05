//! Durable completion events for runner cleanup and capacity refill.

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal_sql::one_row;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CleanupClaim {
    /// Monotonic fencing token for this cleanup attempt.
    pub(crate) generation: i64,
    /// Bounded retry count used only to select a backoff delay.
    pub(crate) attempt: u32,
}

impl Journal {
    /// Commit one exact scale-set completion before its queue message can be acknowledged.
    ///
    /// The deterministic runner name, runner ID, set ID, and request ID must match the launch.
    /// An unrelated runner returns `None` and does not change the journal.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Ownership`] when an event conflicts with a local launch.
    pub(crate) async fn record_runner_completed(
        &self,
        set_id: i64,
        request_id: i64,
        runner_id: i64,
        runner_name: &str,
    ) -> Result<Option<i64>, HostError> {
        if set_id <= 0 || request_id < 0 || runner_id <= 0 {
            return Err(HostError::Ownership);
        }
        let Some(launch_id) = local_launch_id(runner_name) else {
            return Ok(None);
        };
        let expected_assignment = format!("{set_id}:{request_id}");
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = completion_row(&connection, launch_id, &expected_assignment, runner_id).await;
        let ended = if result.is_ok() {
            connection.execute("COMMIT", ()).await
        } else {
            connection.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        self.sync_after(result).await
    }
}

mod claim;

async fn completion_row(
    connection: &turso::Connection,
    launch_id: &str,
    expected_assignment: &str,
    runner_id: i64,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT id, assignment_key, github_runner_id, jit_requested, runner_completed FROM intents WHERE kind = 'launch' AND launch_id = ?1 AND cleanup_proven = 0 ORDER BY id LIMIT 2",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let assignment: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let recorded_runner: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
    let jit_requested: bool = row.get(3).map_err(|_| HostError::Journal)?;
    let completed: bool = row.get(4).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || assignment
            .as_deref()
            .is_some_and(|value| value != expected_assignment)
        || recorded_runner
            .as_deref()
            .is_some_and(|value| value != runner_id.to_string())
        || !jit_requested
    {
        return Err(HostError::Ownership);
    }
    if completed {
        return Ok(Some(id));
    }
    let changed = connection
        .execute(
            "UPDATE intents SET github_runner_id = COALESCE(github_runner_id, ?1), runner_completed = 1 WHERE id = ?2 AND cleanup_proven = 0 AND jit_requested = 1 AND (github_runner_id IS NULL OR github_runner_id = ?1)",
            (runner_id.to_string(), id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    one_row(changed)?;
    Ok(Some(id))
}

fn local_launch_id(runner_name: &str) -> Option<&str> {
    let launch_id = runner_name.strip_prefix('v')?;
    if launch_id.len() == 32
        && launch_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Some(launch_id)
    } else {
        None
    }
}

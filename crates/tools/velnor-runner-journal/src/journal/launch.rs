//! A replay-aware launch intent claim.

use super::{Journal, LaunchClaim, live_id, one_row, token_rejected};
use crate::error::HostError;

impl Journal {
    /// Persist launch identity before AcquireJobs/JIT/Docker effects.
    /// Existing non-null identity fields may only be repeated exactly, and a
    /// runner name cannot be reused by any historical launch generation.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for invalid or conflicting identity or
    /// when the journal cannot commit the update.
    pub async fn bind_launch_identity(
        &self,
        id: i64,
        message_id: Option<i64>,
        runner_request_id: Option<i64>,
        requested_workflow_run_id: Option<i64>,
        requested_job_id: Option<&str>,
        runner_name: &str,
    ) -> Result<(), HostError> {
        if !valid_runner_name(runner_name)
            || message_id.is_some_and(|value| value < 0)
            || runner_request_id.is_some_and(|value| value <= 0)
            || requested_workflow_run_id.is_some_and(|value| value <= 0)
            || requested_job_id.is_some_and(|value| {
                value.is_empty() || value.len() > 256 || value.chars().any(char::is_control)
            })
        {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = bind_launch_identity(
            &conn,
            id,
            message_id,
            runner_request_id,
            requested_workflow_run_id,
            requested_job_id,
            runner_name,
        )
        .await;
        let ended = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        result
    }

    /// Return a fresh launch row only when this subject has no unresolved row.
    ///
    /// The boolean is true for a newly inserted row. A caller must not repeat
    /// acquire or provisioning when an unresolved row already exists.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on invalid input or database failure.
    pub async fn begin_launch(&self, subject: &str) -> Result<(i64, bool), HostError> {
        if token_rejected(subject) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = launch_id(&conn, subject).await;
        let end = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        end.map_err(|_| HostError::Journal)?;
        result
    }

    /// Reserve a launch only if drain has not been committed.
    ///
    /// The drain check and insert share one immediate transaction. Existing
    /// rows are reported without replaying their external effects.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on invalid input or database failure.
    pub async fn begin_launch_if_accepting(&self, subject: &str) -> Result<LaunchClaim, HostError> {
        if token_rejected(subject) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let mut rows = conn
                .query(
                    "SELECT id, state, cleanup_proven, remote_terminal FROM intents WHERE kind = 'launch' AND subject = ?1 ORDER BY id DESC LIMIT 1",
                    [subject],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let latest = if let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                Some((
                    row.get::<i64>(0).map_err(|_| HostError::Journal)?,
                    row.get::<String>(1).map_err(|_| HostError::Journal)?,
                    row.get::<i64>(2).map_err(|_| HostError::Journal)?,
                    row.get::<i64>(3).map_err(|_| HostError::Journal)?,
                ))
            } else {
                None
            };
            drop(rows);
            if let Some((id, state, cleanup, terminal)) = latest
                && state != "failed"
            {
                return match (cleanup, terminal) {
                    (1, 1) => Ok(LaunchClaim::Resolved(id)),
                    (0, 0 | 1) | (1, 0) => Ok(LaunchClaim::Existing(id)),
                    _ => Err(HostError::Journal),
                };
            }
            let mut rows = conn
                .query("SELECT draining FROM controller_state WHERE id = 1", ())
                .await
                .map_err(|_| HostError::Journal)?;
            let draining = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
                .get::<i64>(0)
                .map_err(|_| HostError::Journal)?;
            match draining {
                1 => Ok(LaunchClaim::Draining),
                0 => {
                    conn.execute(
                        "INSERT INTO intents (kind, subject, state) VALUES ('launch', ?1, 'pending')",
                        [subject],
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                    Ok(LaunchClaim::New(conn.last_insert_rowid()))
                }
                _ => Err(HostError::Journal),
            }
        }
        .await;
        let end = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        end.map_err(|_| HostError::Journal)?;
        result
    }
}

async fn bind_launch_identity(
    conn: &turso::Connection,
    id: i64,
    message_id: Option<i64>,
    runner_request_id: Option<i64>,
    requested_workflow_run_id: Option<i64>,
    requested_job_id: Option<&str>,
    runner_name: &str,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT id FROM intents WHERE kind = 'launch' AND runner_name = ?1 AND id != ?2",
            (runner_name, id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let reused = rows.next().await.map_err(|_| HostError::Journal)?.is_some();
    drop(rows);
    if reused {
        return Err(HostError::Journal);
    }
    let changed = conn
        .execute(
            "UPDATE intents SET message_id = COALESCE(message_id, ?1), runner_request_id = COALESCE(runner_request_id, ?2), requested_workflow_run_id = COALESCE(requested_workflow_run_id, ?3), requested_job_id = COALESCE(requested_job_id, ?4), runner_name = COALESCE(runner_name, ?5) WHERE id = ?6 AND kind = 'launch' AND (message_id IS NULL OR ?1 IS NULL OR message_id = ?1) AND (runner_request_id IS NULL OR ?2 IS NULL OR runner_request_id = ?2) AND (requested_workflow_run_id IS NULL OR ?3 IS NULL OR requested_workflow_run_id = ?3) AND (requested_job_id IS NULL OR ?4 IS NULL OR requested_job_id = ?4) AND (runner_name IS NULL OR runner_name = ?5)",
            (
                message_id,
                runner_request_id,
                requested_workflow_run_id,
                requested_job_id.map(str::to_owned),
                runner_name,
                id,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    one_row(changed)
}

fn valid_runner_name(runner_name: &str) -> bool {
    !runner_name.is_empty()
        && runner_name.len() <= 64
        && runner_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

async fn launch_id(conn: &turso::Connection, subject: &str) -> Result<(i64, bool), HostError> {
    if let Some(id) = live_id(conn, "launch", subject).await? {
        return Ok((id, false));
    }
    conn.execute(
        "INSERT INTO intents (kind, subject, state) VALUES ('launch', ?1, 'pending')",
        [subject],
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok((conn.last_insert_rowid(), true))
}

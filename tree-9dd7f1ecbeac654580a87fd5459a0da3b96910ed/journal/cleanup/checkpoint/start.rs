//! Durable runner start intent and cleanup observation checkpoints.

use crate::error::HostError;
use crate::journal::Journal;

use super::{
    RunnerStartObservation,
    read::{cleanup_row, ensure_cleanup_open, step_intended},
};

impl Journal {
    /// Persist the exact runner start observation before removing its container.
    ///
    /// Replays of the same observation are idempotent; conflicting evidence
    /// stays unresolved. `NeverStarted` requires an explicit durable no-start
    /// intent state, while legacy rows must be classified `MayHaveStarted`.
    ///
    /// # Errors
    ///
    /// Returns `HostError::Journal` for a missing fence, mismatched runner,
    /// incompatible start intent, conflicting replay, or database failure.
    pub async fn record_cleanup_runner_start(
        &self,
        launch_id: i64,
        runner_container_id: &str,
        observation: RunnerStartObservation,
    ) -> Result<(), HostError> {
        if self.read_only || launch_id <= 0 || !super::validation::container_id(runner_container_id)
        {
            return Err(HostError::Journal);
        }
        self.with_immediate(async |conn| {
            ensure_cleanup_open(conn, launch_id).await?;
            let mut rows = conn
                .query(
                    "SELECT i.runner_start_state, i.docker_id, c.runner_start_observation, c.post_action_disposition FROM intents i JOIN worker_cleanup c ON c.launch_id = i.id WHERE i.id = ?1 AND i.kind = 'launch'",
                    [launch_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?;
            let intent = row
                .get::<Option<String>>(0)
                .map_err(|_| HostError::Journal)?;
            let runner = row
                .get::<Option<String>>(1)
                .map_err(|_| HostError::Journal)?;
            let existing = row
                .get::<Option<String>>(2)
                .map_err(|_| HostError::Journal)?;
            let post_action: String = row.get(3).map_err(|_| HostError::Journal)?;
            if runner.as_deref() != Some(runner_container_id)
                || (observation == RunnerStartObservation::NeverStarted
                    && intent.as_deref() != Some("not_requested"))
                || (observation == RunnerStartObservation::MayHaveStarted
                    && intent.as_deref() == Some("not_requested"))
                || (post_action == "not_run"
                    && observation != RunnerStartObservation::NeverStarted)
            {
                return Err(HostError::Journal);
            }
            if let Some(value) = existing {
                return if RunnerStartObservation::parse(&value)? == observation {
                    Ok(())
                } else {
                    Err(HostError::Journal)
                };
            }
            if step_intended(conn, launch_id, "runner-termination").await? {
                return Err(HostError::Journal);
            }
            drop(rows);
            let changed = conn
                .execute(
                    "UPDATE worker_cleanup SET runner_start_observation = ?1 WHERE launch_id = ?2 AND runner_start_observation IS NULL",
                    (observation.as_str(), launch_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    /// Read a previously persisted runner start observation for recovery.
    ///
    /// # Errors
    ///
    /// Returns `HostError::Journal` when the cleanup fence is missing or the
    /// checkpoint cannot be decoded.
    pub async fn cleanup_runner_start(
        &self,
        launch_id: i64,
    ) -> Result<Option<RunnerStartObservation>, HostError> {
        let conn = self.connection().await?;
        ensure_cleanup_open(&conn, launch_id).await?;
        Ok(cleanup_row(&conn, launch_id)
            .await?
            .ok_or(HostError::Journal)?
            .runner_start_observation)
    }
}

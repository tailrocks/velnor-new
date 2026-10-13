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
}

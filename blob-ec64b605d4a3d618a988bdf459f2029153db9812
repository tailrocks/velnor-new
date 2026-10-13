//! One-way durable transitions before remote assignment and runner creation.

#[cfg(test)]
use crate::error::HostError;
#[cfg(test)]
use crate::journal::Journal;
#[cfg(test)]
use crate::journal_sql::intent_row;
#[cfg(test)]
use crate::reconcile::IntentRow;

#[cfg(test)]
impl Journal {
    /// Read one durable launch row.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is absent or malformed.
    pub(crate) async fn intent(&self, id: i64) -> Result<IntentRow, HostError> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT id, kind, subject, state, docker_id, dind_id, github_runner_id, cleanup_proven, launch_id, assignment_key, seed_generation_id, acquire_attempted, acquire_resolved, acquired, jit_requested, runner_completed, worker_volume, scale_set_id, runner_request_id, runner_name, docker_engine_id, launch_phase FROM intents WHERE id = ?1",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        intent_row(&row)
    }
}

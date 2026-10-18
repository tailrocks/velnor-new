//! Connection-scoped launch rows and immutable worker identity.

use crate::error::HostError;
use crate::journal::sql::intent_row;
use crate::reconcile::IntentRow;

use super::super::ConnectionJournal;
use super::super::storage::read_transaction;

impl ConnectionJournal {
    pub(crate) async fn rows(&self) -> Result<Vec<IntentRow>, HostError> {
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT id, kind, subject, state, docker_id, github_runner_id, cleanup_proven, dind_id, worker_volume, scale_set_id, runner_request_id, runner_name, docker_engine_id, launch_phase, launch_id, assignment_key, seed_generation_id, acquire_attempted, acquire_resolved, acquired, jit_requested, runner_completed FROM connection_intents WHERE connection_id = ?1 AND scale_set_id = ?2 ORDER BY id",
                    (self.connection_id.as_str(), set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut result = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                result.push(intent_row(&row)?);
            }
            Ok(result)
        })
        .await
    }

    /// Read one launch row inside this connection and Scale Set.
    pub(crate) async fn intent(&self, id: i64) -> Result<IntentRow, HostError> {
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT id, kind, subject, state, docker_id, github_runner_id, cleanup_proven, dind_id, worker_volume, scale_set_id, runner_request_id, runner_name, docker_engine_id, launch_phase, launch_id, assignment_key, seed_generation_id, acquire_attempted, acquire_resolved, acquired, jit_requested, runner_completed FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3",
                    (id, self.connection_id.as_str(), set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?;
            intent_row(&row)
        })
        .await
    }
}

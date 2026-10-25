//! Read due cleanup rows for one repository connection.

use crate::error::HostError;
use crate::journal::sql::intent_row;
use crate::journal::{CompletedLaunch, CompletionIdentity};

use super::super::ConnectionJournal;
use super::super::storage::read_transaction;

impl ConnectionJournal {
    /// Read this connection's due runner completions in retry order.
    pub(crate) async fn due_completed_launches(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<Vec<CompletedLaunch>, HostError> {
        if now < 0 {
            return Err(HostError::Journal);
        }
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT i.id, i.kind, i.subject, i.state, i.docker_id, i.github_runner_id, i.cleanup_proven, i.dind_id, i.worker_volume, i.scale_set_id, i.runner_request_id, i.runner_name, i.docker_engine_id, i.launch_phase, i.launch_id, i.assignment_key, i.seed_generation_id, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, i.runner_completed, c.scale_set_id, c.runner_request_id, c.runner_id, c.runner_name, c.runner_absent FROM connection_intents AS i JOIN connection_completion_cleanup AS c ON c.intent_id = i.id AND c.connection_id = i.connection_id WHERE i.connection_id = ?1 AND i.scale_set_id = ?2 AND i.kind = 'launch' AND i.cleanup_proven = 0 AND i.scale_set_id = c.scale_set_id AND i.runner_name = c.runner_name AND (i.runner_request_id IS NULL OR i.runner_request_id = c.runner_request_id) AND c.retry_after <= ?3 AND c.lease_until <= ?3 ORDER BY c.retry_after, i.id LIMIT ?4",
                    (self.connection_id.as_str(), set_id, now, i64::from(limit)),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                out.push(CompletedLaunch {
                    intent: intent_row(&row)?,
                    identity: CompletionIdentity {
                        scale_set_id: row.get(22).map_err(|_| HostError::Journal)?,
                        runner_request_id: row.get(23).map_err(|_| HostError::Journal)?,
                        runner_id: row.get(24).map_err(|_| HostError::Journal)?,
                        runner_name: row.get(25).map_err(|_| HostError::Journal)?,
                        runner_absent: row.get(26).map_err(|_| HostError::Journal)?,
                    },
                });
            }
            Ok(out)
        })
        .await
    }
}

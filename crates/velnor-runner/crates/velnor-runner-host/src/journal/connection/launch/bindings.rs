//! Immutable worker identity binding and lookup.

use crate::error::HostError;
use crate::journal::sql::token_rejected;
use crate::launch_identity::LaunchIdentity;

use super::super::ConnectionJournal;
use super::super::storage::{in_transaction, read_transaction};
use super::helpers::{bind_ids, same_optional};

impl ConnectionJournal {
    pub(crate) async fn bind(
        &self,
        id: i64,
        docker_id: Option<&str>,
        github_runner_id: Option<&str>,
    ) -> Result<(), HostError> {
        if docker_id.is_some_and(token_rejected) || github_runner_id.is_some_and(token_rejected) {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            bind_ids(
                connection,
                id,
                self.connection_id.as_str(),
                docker_id,
                github_runner_id,
                None,
            )
            .await
        })
        .await
    }

    /// Bind immutable runner and private DinD container IDs.
    pub(crate) async fn bind_worker(
        &self,
        id: i64,
        runner_id: Option<&str>,
        dind_id: Option<&str>,
    ) -> Result<(), HostError> {
        if runner_id.is_some_and(token_rejected) || dind_id.is_some_and(token_rejected) {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            bind_ids(
                connection,
                id,
                self.connection_id.as_str(),
                runner_id,
                None,
                dind_id,
            )
            .await
        })
        .await
    }

    /// Bind the worker volume name before Docker creates it.
    pub(crate) async fn bind_worker_volume(&self, id: i64, volume: &str) -> Result<(), HostError> {
        if token_rejected(volume) {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET worker_volume = COALESCE(worker_volume, ?1) WHERE id = ?2 AND connection_id = ?3 AND kind = 'launch' AND cleanup_proven = 0 AND (worker_volume IS NULL OR worker_volume = ?1)",
                    (volume, id, self.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 || same_optional(connection, id, self.connection_id.as_str(), "worker_volume", volume).await? {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    /// Read the immutable launch identity after the global engine is bound.
    pub(crate) async fn launch_identity(&self, id: i64) -> Result<LaunchIdentity, HostError> {
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT launch_id FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch'",
                    (id, self.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let launch_id: Option<String> = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
                .get(0)
                .map_err(|_| HostError::Journal)?;
            let launch_id = launch_id.ok_or(HostError::Journal)?;
            let instance_id = crate::journal_schema::instance_id(connection).await?;
            let engine_id = crate::journal_schema::engine_id(connection).await?;
            LaunchIdentity::new(&instance_id, id, &launch_id, &engine_id)
        })
        .await
    }

    /// Bind the runner and DinD resource IDs as one immutable pair.
    pub(crate) async fn bind_pair(
        &self,
        id: i64,
        runner_id: &str,
        dind_id: &str,
    ) -> Result<(), HostError> {
        if token_rejected(runner_id) || token_rejected(dind_id) {
            return Err(HostError::Journal);
        }
        self.bind_worker(id, Some(runner_id), Some(dind_id)).await
    }

    /// Pin one immutable action seed generation to this launch.
    pub(crate) async fn bind_seed_generation(
        &self,
        id: i64,
        generation_id: &str,
    ) -> Result<(), HostError> {
        if token_rejected(generation_id) {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET seed_generation_id = COALESCE(seed_generation_id, ?1) WHERE id = ?2 AND connection_id = ?3 AND kind = 'launch' AND cleanup_proven = 0 AND (seed_generation_id IS NULL OR seed_generation_id = ?1)",
                    (generation_id, id, self.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 || same_optional(connection, id, self.connection_id.as_str(), "seed_generation_id", generation_id).await? {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }
}

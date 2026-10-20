//! Completion cleanup proofs fenced to the owning connection and lease.

use crate::error::HostError;
use crate::journal::sql::token_rejected;

use super::super::ConnectionJournal;
use super::super::storage::in_transaction;
use super::claim::{current_claim, unix_seconds};

#[path = "proof/settled.rs"]
mod settled;

impl ConnectionJournal {
    #[cfg(test)]
    pub(crate) async fn terminal_settlement_matches_at(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            settled::matches_terminal_settlement(
                connection,
                self.connection_id().as_str(),
                set_id,
                id,
                generation,
                self.binding_digest(),
            )
            .await
        })
        .await
    }

    #[cfg(test)]
    pub(crate) async fn completion_cleanup_schedule_at(
        &self,
        id: i64,
    ) -> Result<Option<(i64, i64)>, HostError> {
        if id <= 0 {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT lease_until, retry_after FROM connection_completion_cleanup WHERE intent_id = ?1 AND connection_id = ?2 AND scale_set_id = ?3",
                    (id, self.connection_id().as_str(), set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let schedule = (
                row.get(0).map_err(|_| HostError::Journal)?,
                row.get(1).map_err(|_| HostError::Journal)?,
            );
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }
            Ok(Some(schedule))
        })
        .await
    }

    /// Bind verified local container IDs under a current cleanup lease.
    pub(crate) async fn bind_completion_containers(
        &self,
        id: i64,
        generation: i64,
        runner_container_id: Option<&str>,
        dind_id: Option<&str>,
    ) -> Result<bool, HostError> {
        self.bind_completion_containers_with(
            id,
            generation,
            runner_container_id,
            dind_id,
            unix_seconds,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn bind_completion_containers_at(
        &self,
        id: i64,
        generation: i64,
        runner_container_id: Option<&str>,
        dind_id: Option<&str>,
        now: i64,
    ) -> Result<bool, HostError> {
        self.bind_completion_containers_with(id, generation, runner_container_id, dind_id, || {
            Ok(now)
        })
        .await
    }

    async fn bind_completion_containers_with(
        &self,
        id: i64,
        generation: i64,
        runner_container_id: Option<&str>,
        dind_id: Option<&str>,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        if id <= 0
            || generation <= 0
            || runner_container_id.is_some_and(token_rejected)
            || dind_id.is_some_and(token_rejected)
        {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let now = clock()?;
            if now < 0
                || !current_claim(
                    connection,
                    self.connection_id().as_str(),
                    set_id,
                    id,
                    generation,
                    now,
                )
                .await?
            {
                return Ok(false);
            }
            if runner_container_id.is_none() && dind_id.is_none() {
                return Ok(true);
            }
            bind_ids(
                connection,
                self.connection_id().as_str(),
                set_id,
                id,
                runner_container_id,
                dind_id,
            )
            .await
        })
        .await
    }

    /// Persist exact official runner absence under the current cleanup lease.
    pub(crate) async fn record_completion_runner_absent(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        self.record_completion_runner_absent_with(id, generation, unix_seconds)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn record_completion_runner_absent_at(
        &self,
        id: i64,
        generation: i64,
        now: i64,
    ) -> Result<bool, HostError> {
        self.record_completion_runner_absent_with(id, generation, || Ok(now))
            .await
    }

    async fn record_completion_runner_absent_with(
        &self,
        id: i64,
        generation: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let now = clock()?;
            if now < 0 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_completion_cleanup SET runner_absent = 1 WHERE intent_id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND claim_generation = ?4 AND lease_until > ?5 AND EXISTS (SELECT 1 FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND cleanup_proven = 0)",
                    (id, self.connection_id().as_str(), set_id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(changed == 1)
        })
        .await
    }

    /// Commit completion cleanup only after exact runner absence under the current lease.
    pub(crate) async fn record_completion_cleanup(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        self.record_completion_cleanup_with(id, generation, unix_seconds)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn record_completion_cleanup_at(
        &self,
        id: i64,
        generation: i64,
        now: i64,
    ) -> Result<bool, HostError> {
        self.record_completion_cleanup_with(id, generation, || Ok(now))
            .await
    }

    async fn record_completion_cleanup_with(
        &self,
        id: i64,
        generation: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let now = clock()?;
            if now < 0 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET cleanup_proven = 1 WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND kind = 'launch' AND cleanup_proven = 0 AND EXISTS (SELECT 1 FROM connection_completion_cleanup AS c WHERE c.intent_id = ?1 AND c.connection_id = ?2 AND c.scale_set_id = ?3 AND c.claim_generation = ?4 AND c.lease_until > ?5 AND c.runner_absent = 1)",
                    (id, self.connection_id().as_str(), set_id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 0 {
                if !settled::matches_terminal_settlement(
                    connection,
                    self.connection_id().as_str(),
                    set_id,
                    id,
                    generation,
                    self.binding_digest(),
                )
                .await?
                {
                    return Ok(false);
                }
                return settled::retire_terminal_claim(
                    connection,
                    self.connection_id().as_str(),
                    set_id,
                    id,
                    generation,
                )
                .await;
            }
            crate::journal::request_lifecycle::physical::mark_runner_cleaned_after_cleanup(
                connection,
                self.connection_id().as_str(),
                id,
            )
            .await?;
            let retired = connection
                .execute(
                    "UPDATE connection_completion_cleanup SET retry_after = 0, lease_until = 0 WHERE intent_id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND claim_generation = ?4",
                    (id, self.connection_id().as_str(), set_id, generation),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if retired != 1 {
                return Err(HostError::Journal);
            }
            Ok(true)
        })
        .await
    }
}

async fn bind_ids(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    id: i64,
    runner_container_id: Option<&str>,
    dind_id: Option<&str>,
) -> Result<bool, HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_intents SET docker_id = COALESCE(docker_id, ?1), dind_id = COALESCE(dind_id, ?2) WHERE id = ?3 AND connection_id = ?4 AND scale_set_id = ?5 AND kind = 'launch' AND cleanup_proven = 0 AND (?1 IS NULL OR docker_id IS NULL OR docker_id = ?1) AND (?2 IS NULL OR dind_id IS NULL OR dind_id = ?2)",
            (runner_container_id, dind_id, id, connection_id, scale_set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        return Ok(true);
    }
    let mut rows = connection
        .query(
            "SELECT docker_id, dind_id FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3",
            (id, connection_id, scale_set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let stored_runner: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
    let stored_dind: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    Ok(
        runner_container_id.is_none_or(|value| stored_runner.as_deref() == Some(value))
            && dind_id.is_none_or(|value| stored_dind.as_deref() == Some(value)),
    )
}

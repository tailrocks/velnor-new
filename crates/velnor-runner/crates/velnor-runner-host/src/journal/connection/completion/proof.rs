//! Completion cleanup proofs fenced to the owning connection and lease.

use super::super::ConnectionJournal;
use super::super::storage::in_transaction;
use super::claim::unix_seconds;
use crate::error::HostError;

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

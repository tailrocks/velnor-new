// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::ConnectionJournal;
use super::super::storage::in_transaction;
use super::claim::unix_seconds;
use crate::error::HostError;

#[path = "proof/settled.rs"]
mod settled;

impl ConnectionJournal {
    pub(crate) async fn record_completion_runner_absent(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        self.record_completion_runner_absent_with(id, generation, unix_seconds)
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

    pub(crate) async fn record_completion_cleanup(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        self.record_completion_cleanup_with(id, generation, unix_seconds)
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

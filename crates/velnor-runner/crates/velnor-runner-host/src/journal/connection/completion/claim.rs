//! Connection-scoped completion cleanup leases and bounded effect renewal.

use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::HostError;
use crate::journal::CleanupClaim;

use super::super::ConnectionJournal;
use super::super::storage::in_transaction;

const MAX_ATTEMPTS: i64 = 5;

impl ConnectionJournal {
    pub(crate) fn completion_state_identity(&self) -> String {
        format!(
            "{}#{}",
            self.journal.completion_state_identity(),
            self.connection_id().as_str()
        )
    }

    /// Check whether this generation still owns an unexpired cleanup claim.
    pub(crate) async fn completion_cleanup_claim_current(
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
            current_claim(
                connection,
                self.connection_id().as_str(),
                set_id,
                id,
                generation,
                unix_seconds()?,
            )
            .await
        })
        .await
    }

    /// Claim one due cleanup row before bounded HTTP or Docker work.
    pub(crate) async fn claim_completion_cleanup(
        &self,
        id: i64,
        lease_seconds: i64,
    ) -> Result<Option<CleanupClaim>, HostError> {
        self.claim_completion_cleanup_with(id, lease_seconds, unix_seconds)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn claim_completion_cleanup_at(
        &self,
        id: i64,
        now: i64,
        lease_seconds: i64,
    ) -> Result<Option<CleanupClaim>, HostError> {
        self.claim_completion_cleanup_with(id, lease_seconds, || Ok(now))
            .await
    }

    async fn claim_completion_cleanup_with(
        &self,
        id: i64,
        lease_seconds: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<Option<CleanupClaim>, HostError> {
        if id <= 0 || lease_seconds <= 0 {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let now = clock()?;
            if now < 0 {
                return Err(HostError::Journal);
            }
            let lease_until = now.checked_add(lease_seconds).ok_or(HostError::Journal)?;
            claim_row(
                connection,
                self.connection_id().as_str(),
                set_id,
                id,
                now,
                lease_until,
            )
            .await
        })
        .await
    }

    /// Schedule retry after cleanup failed to prove release.
    pub(crate) async fn retry_completion_cleanup(
        &self,
        id: i64,
        generation: i64,
        retry_delay_seconds: i64,
    ) -> Result<bool, HostError> {
        self.retry_completion_cleanup_with(id, generation, retry_delay_seconds, unix_seconds)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn retry_completion_cleanup_at(
        &self,
        id: i64,
        generation: i64,
        now: i64,
        retry_after: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 || now < 0 || retry_after < now {
            return Err(HostError::Journal);
        }
        self.retry_completion_cleanup_with(id, generation, retry_after - now, || Ok(now))
            .await
    }

    async fn retry_completion_cleanup_with(
        &self,
        id: i64,
        generation: i64,
        retry_delay_seconds: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 || retry_delay_seconds < 0 {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let now = clock()?;
            if now < 0 {
                return Err(HostError::Journal);
            }
            let retry_after = now
                .checked_add(retry_delay_seconds)
                .ok_or(HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE connection_completion_cleanup SET retry_after = ?1, lease_until = 0 WHERE intent_id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND claim_generation = ?5 AND lease_until > ?6 AND EXISTS (SELECT 1 FROM connection_intents WHERE id = ?2 AND connection_id = ?3 AND kind = 'launch' AND cleanup_proven = 0)",
                    (retry_after, id, self.connection_id().as_str(), set_id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(changed == 1)
        })
        .await
    }

    #[cfg(test)]
    pub(crate) async fn completion_cleanup_claim_current_at(
        &self,
        id: i64,
        generation: i64,
        now: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 || now < 0 {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            current_claim(
                connection,
                self.connection_id().as_str(),
                set_id,
                id,
                generation,
                now,
            )
            .await
        })
        .await
    }

    /// Extend the current claim, commit, then run exactly one bounded effect.
    pub(crate) async fn run_completion_cleanup_effect<T, F, Fut>(
        &self,
        id: i64,
        generation: i64,
        effect_window_seconds: i64,
        effect: F,
    ) -> Result<Option<T>, HostError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, HostError>>,
    {
        self.run_completion_cleanup_effect_with(
            id,
            generation,
            effect_window_seconds,
            unix_seconds,
            effect,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn run_completion_cleanup_effect_at<T, F, Fut>(
        &self,
        id: i64,
        generation: i64,
        now: i64,
        effect_window_seconds: i64,
        effect: F,
    ) -> Result<Option<T>, HostError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, HostError>>,
    {
        self.run_completion_cleanup_effect_with(
            id,
            generation,
            effect_window_seconds,
            || Ok(now),
            effect,
        )
        .await
    }

    async fn run_completion_cleanup_effect_with<T, F, Fut>(
        &self,
        id: i64,
        generation: i64,
        effect_window_seconds: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
        effect: F,
    ) -> Result<Option<T>, HostError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, HostError>>,
    {
        if id <= 0 || generation <= 0 || effect_window_seconds <= 0 {
            return Err(HostError::Journal);
        }
        let allowed = in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let now = clock()?;
            if now < 0 {
                return Err(HostError::Journal);
            }
            let lease_until = now
                .checked_add(effect_window_seconds)
                .ok_or(HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE connection_completion_cleanup SET lease_until = MAX(lease_until, ?1) WHERE intent_id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND claim_generation = ?5 AND lease_until > ?6 AND EXISTS (SELECT 1 FROM connection_intents WHERE id = ?2 AND connection_id = ?3 AND kind = 'launch' AND cleanup_proven = 0)",
                    (lease_until, id, self.connection_id().as_str(), set_id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(changed == 1)
        })
        .await?;
        if allowed {
            effect().await.map(Some)
        } else {
            Ok(None)
        }
    }
}

async fn claim_row(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    id: i64,
    now: i64,
    lease_until: i64,
) -> Result<Option<CleanupClaim>, HostError> {
    let mut rows = connection
        .query(
            "SELECT c.attempts, c.claim_generation, c.retry_after, c.lease_until FROM connection_completion_cleanup AS c JOIN connection_intents AS i ON i.id = c.intent_id AND i.connection_id = c.connection_id WHERE c.intent_id = ?1 AND c.connection_id = ?2 AND c.scale_set_id = ?3 AND i.kind = 'launch' AND i.cleanup_proven = 0",
            (id, connection_id, scale_set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let attempts: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let generation: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let retry_after: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    let lease: i64 = row.get(3).map_err(|_| HostError::Journal)?;
    if attempts < 0 || generation < 0 || retry_after < 0 || lease < 0 {
        return Err(HostError::Journal);
    }
    if retry_after > now || lease > now {
        return Ok(None);
    }
    let next_generation = generation.checked_add(1).ok_or(HostError::Journal)?;
    let next_attempt = attempts.saturating_add(1).min(MAX_ATTEMPTS);
    let changed = connection
        .execute(
            "UPDATE connection_completion_cleanup SET attempts = ?1, claim_generation = ?2, lease_until = ?3 WHERE intent_id = ?4 AND connection_id = ?5 AND scale_set_id = ?6 AND claim_generation = ?7 AND retry_after <= ?8 AND lease_until <= ?8",
            (next_attempt, next_generation, lease_until, id, connection_id, scale_set_id, generation, now),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Ok(None);
    }
    Ok(Some(CleanupClaim {
        generation: next_generation,
        attempt: u32::try_from(next_attempt).map_err(|_| HostError::Journal)?,
    }))
}

pub(super) async fn current_claim(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    id: i64,
    generation: i64,
    now: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM connection_completion_cleanup AS c JOIN connection_intents AS i ON i.id = c.intent_id AND i.connection_id = c.connection_id WHERE c.intent_id = ?1 AND c.connection_id = ?2 AND c.scale_set_id = ?3 AND c.claim_generation = ?4 AND c.lease_until > ?5 AND i.kind = 'launch' AND i.cleanup_proven = 0",
            (id, connection_id, scale_set_id, generation, now),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

pub(super) fn unix_seconds() -> Result<i64, HostError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| HostError::Journal)
}

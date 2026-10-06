//! Completion cleanup claim, retry, and bounded-effect authorization.

use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::HostError;
use crate::journal::Journal;

use super::{CleanupClaim, finish_transaction};

const MAX_COMPLETION_ATTEMPTS: i64 = 5;

impl Journal {
    /// Claim one due completion row before bounded HTTP or Docker work.
    pub(crate) async fn claim_completion_cleanup(
        &self,
        id: i64,
        lease_seconds: i64,
    ) -> Result<Option<CleanupClaim>, HostError> {
        if id <= 0 || lease_seconds <= 0 {
            return Err(HostError::Journal);
        }
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
        if id <= 0 || now < 0 || lease_seconds <= 0 {
            return Err(HostError::Journal);
        }
        self.claim_completion_cleanup_with(id, lease_seconds, || Ok(now))
            .await
    }

    async fn claim_completion_cleanup_with(
        &self,
        id: i64,
        lease_seconds: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<Option<CleanupClaim>, HostError> {
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let now = clock()?;
            if now < 0 {
                return Err(HostError::Journal);
            }
            let lease_until = now.checked_add(lease_seconds).ok_or(HostError::Journal)?;
            claim_cleanup_row(&connection, id, now, lease_until).await
        }
        .await;
        finish_transaction(&connection, result).await
    }

    /// Schedule retry after cleanup failed to prove both release facts.
    pub(crate) async fn retry_completion_cleanup(
        &self,
        id: i64,
        generation: i64,
        retry_delay_seconds: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 || retry_delay_seconds < 0 {
            return Err(HostError::Journal);
        }
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
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let now = clock()?;
            if now < 0 {
                return Err(HostError::Journal);
            }
            let retry_after = now
                .checked_add(retry_delay_seconds)
                .ok_or(HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE completion_cleanup SET retry_after = ?1, lease_until = 0 WHERE intent_id = ?2 AND claim_generation = ?3 AND lease_until > ?4 AND EXISTS (SELECT 1 FROM intents WHERE id = ?2 AND kind = 'launch' AND cleanup_proven = 0)",
                    (retry_after, id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(changed == 1)
        }
        .await;
        finish_transaction(&connection, result).await
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
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = current_claim(&connection, id, generation, now).await;
        finish_transaction(&connection, result).await
    }

    /// Renew a current claim to cover one bounded effect before invoking its factory.
    ///
    /// The renewal commits before external I/O. Expired or stale generations cannot
    /// extend themselves, and a second journal handle cannot claim during the window.
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
        self.run_completion_cleanup_effect_with_clock(
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
        self.run_completion_cleanup_effect_with_clock(
            id,
            generation,
            effect_window_seconds,
            || Ok(now),
            effect,
        )
        .await
    }

    async fn run_completion_cleanup_effect_with_clock<T, F, Fut>(
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
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let now = clock()?;
            if now < 0 {
                return Err(HostError::Journal);
            }
            let lease_until = now
                .checked_add(effect_window_seconds)
                .ok_or(HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE completion_cleanup SET lease_until = MAX(lease_until, ?1) WHERE intent_id = ?2 AND claim_generation = ?3 AND lease_until > ?4 AND EXISTS (SELECT 1 FROM intents WHERE id = ?2 AND kind = 'launch' AND cleanup_proven = 0)",
                    (lease_until, id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(changed == 1)
        }
        .await;
        if !finish_transaction(&connection, result).await? {
            return Ok(None);
        }
        effect().await.map(Some)
    }
}

async fn claim_cleanup_row(
    connection: &turso::Connection,
    id: i64,
    now: i64,
    lease_until: i64,
) -> Result<Option<CleanupClaim>, HostError> {
    let mut rows = connection
        .query(
            "SELECT c.attempts, c.claim_generation, c.retry_after, c.lease_until FROM completion_cleanup AS c JOIN intents AS i ON i.id = c.intent_id WHERE c.intent_id = ?1 AND i.kind = 'launch' AND i.cleanup_proven = 0",
            [id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let attempts: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let generation: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let retry_after: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    let current_lease: i64 = row.get(3).map_err(|_| HostError::Journal)?;
    if attempts < 0 || generation < 0 {
        return Err(HostError::Journal);
    }
    if retry_after > now || current_lease > now {
        return Ok(None);
    }
    let next_generation = generation.checked_add(1).ok_or(HostError::Journal)?;
    let next_attempt = attempts.saturating_add(1).min(MAX_COMPLETION_ATTEMPTS);
    let changed = connection
        .execute(
            "UPDATE completion_cleanup SET attempts = ?1, claim_generation = ?2, lease_until = ?3 WHERE intent_id = ?4 AND claim_generation = ?5 AND retry_after <= ?6 AND lease_until <= ?6",
            (next_attempt, next_generation, lease_until, id, generation, now),
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
    id: i64,
    generation: i64,
    now: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM completion_cleanup AS c JOIN intents AS i ON i.id = c.intent_id WHERE c.intent_id = ?1 AND c.claim_generation = ?2 AND c.lease_until > ?3 AND i.kind = 'launch' AND i.cleanup_proven = 0",
            (id, generation, now),
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

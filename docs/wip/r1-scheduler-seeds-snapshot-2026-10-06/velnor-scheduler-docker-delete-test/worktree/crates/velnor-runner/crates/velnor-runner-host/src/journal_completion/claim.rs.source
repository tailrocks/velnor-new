//! Durable completion cleanup claims and fencing transitions.

use crate::error::HostError;
use crate::journal::Journal;
use crate::reconcile::IntentRow;

use super::CleanupClaim;
#[path = "claim_current.rs"]
mod claim_current;
#[path = "claim_effect.rs"]
mod claim_effect;
#[path = "claim_sql.rs"]
mod claim_sql;
use claim_sql::{claim_cleanup_row, finish_transition, unix_seconds};

impl Journal {
    /// Return rows whose completion event is durable but whose cleanup is not proven.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the journal cannot be read.
    pub(crate) async fn completed_launches(&self) -> Result<Vec<IntentRow>, HostError> {
        Ok(self
            .rows()
            .await?
            .into_iter()
            .filter(|row| row.kind == "launch" && row.runner_completed && !row.cleanup_proven)
            .collect())
    }

    /// Return due completion rows in fair retry order.
    pub(crate) async fn due_completed_launches(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<Vec<IntentRow>, HostError> {
        let limit = i64::from(limit);
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT i.id, i.kind, i.subject, i.state, i.docker_id, i.dind_id, i.github_runner_id, i.cleanup_proven, i.launch_id, i.assignment_key, i.seed_generation_id, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, i.runner_completed FROM intents AS i LEFT JOIN completion_cleanup AS c ON c.intent_id = i.id WHERE i.kind = 'launch' AND i.runner_completed = 1 AND i.cleanup_proven = 0 AND COALESCE(c.retry_after, 0) <= ?1 AND COALESCE(c.lease_until, 0) <= ?1 ORDER BY COALESCE(c.retry_after, 0), i.id LIMIT ?2",
                (now, limit),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
            out.push(crate::journal_sql::intent_row(&row)?);
        }
        Ok(out)
    }

    /// Claim one due row before network or Docker work starts.
    pub(crate) async fn claim_completion_cleanup(
        &self,
        id: i64,
        lease_seconds: i64,
    ) -> Result<Option<CleanupClaim>, HostError> {
        if id <= 0 || lease_seconds <= 0 {
            return Err(HostError::Journal);
        }
        self.claim_completion_cleanup_with(id, || {
            let now = unix_seconds()?;
            let lease_until = now.checked_add(lease_seconds).ok_or(HostError::Journal)?;
            Ok((now, lease_until))
        })
        .await
    }

    #[cfg(test)]
    pub(crate) async fn claim_completion_cleanup_at(
        &self,
        id: i64,
        now: i64,
        lease_until: i64,
    ) -> Result<Option<CleanupClaim>, HostError> {
        if id <= 0 || lease_until <= now {
            return Err(HostError::Journal);
        }
        self.claim_completion_cleanup_with(id, || Ok((now, lease_until)))
            .await
    }

    async fn claim_completion_cleanup_with(
        &self,
        id: i64,
        clock: impl FnOnce() -> Result<(i64, i64), HostError>,
    ) -> Result<Option<CleanupClaim>, HostError> {
        let _intent = self.completion_lock(id)?.lock_owned().await;
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let (now, lease_until) = clock()?;
            if now < 0 || lease_until <= now {
                return Err(HostError::Journal);
            }
            claim_cleanup_row(&connection, id, now, lease_until).await
        }
        .await;
        finish_transition(&connection, &result).await?;
        self.sync_after(result).await
    }

    /// Set durable retry time after one cleanup attempt did not prove release.
    pub(crate) async fn retry_completion_cleanup(
        &self,
        id: i64,
        generation: i64,
        retry_delay_seconds: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 || retry_delay_seconds < 0 {
            return Err(HostError::Journal);
        }
        self.retry_completion_cleanup_with(id, generation, || {
            let now = unix_seconds()?;
            let retry_after = now
                .checked_add(retry_delay_seconds)
                .ok_or(HostError::Journal)?;
            Ok((now, retry_after))
        })
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
        self.retry_completion_cleanup_with(id, generation, || Ok((now, retry_after)))
            .await
    }

    async fn retry_completion_cleanup_with(
        &self,
        id: i64,
        generation: i64,
        clock: impl FnOnce() -> Result<(i64, i64), HostError>,
    ) -> Result<bool, HostError> {
        let _intent = self.completion_lock(id)?.lock_owned().await;
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let (now, retry_after) = clock()?;
            if now < 0 || retry_after < now {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE completion_cleanup SET retry_after = ?1, lease_until = 0 WHERE intent_id = ?2 AND claim_generation = ?3 AND lease_until > ?4",
                    (retry_after, id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(changed == 1)
        }
        .await;
        finish_transition(&connection, &result).await?;
        self.sync_after(result).await
    }

    /// Mark worker and official-runner cleanup as proven before archive-lease retirement.
    ///
    /// This marker lets restart recovery finish lease retirement after a crash without
    /// repeating Docker or GitHub cleanup.
    pub(crate) async fn mark_completion_worker_cleanup_proven(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 {
            return Err(HostError::Journal);
        }
        self.mark_completion_worker_cleanup_proven_with(id, generation, unix_seconds)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn mark_completion_worker_cleanup_proven_at(
        &self,
        id: i64,
        generation: i64,
        now: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 || now < 0 {
            return Err(HostError::Journal);
        }
        self.mark_completion_worker_cleanup_proven_with(id, generation, || Ok(now))
            .await
    }

    #[cfg(test)]
    pub(crate) async fn mark_completion_worker_cleanup_proven_with_clock_for_test(
        &self,
        id: i64,
        generation: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        self.mark_completion_worker_cleanup_proven_with(id, generation, clock)
            .await
    }

    async fn mark_completion_worker_cleanup_proven_with(
        &self,
        id: i64,
        generation: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        let _intent = self.completion_lock(id)?.lock_owned().await;
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
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
            let changed = connection
                .execute(
                    "UPDATE intents SET worker_cleanup_proven = 1 WHERE id = ?1 AND kind = 'launch' AND runner_completed = 1 AND cleanup_proven = 0 AND EXISTS (SELECT 1 FROM completion_cleanup AS c WHERE c.intent_id = intents.id AND c.claim_generation = ?2 AND c.lease_until > ?3)",
                    (id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(changed == 1)
        }
        .await;
        finish_transition(&connection, &result).await?;
        self.sync_after(result).await
    }

    /// Commit final completion cleanup only for the current unexpired claim.
    ///
    /// This transition frees the occupied slot and removes the claim atomically.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the transaction or lineage sync fails.
    pub(crate) async fn record_completion_cleanup(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 {
            return Err(HostError::Journal);
        }
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
        if id <= 0 || generation <= 0 || now < 0 {
            return Err(HostError::Journal);
        }
        self.record_completion_cleanup_with(id, generation, || Ok(now))
            .await
    }

    #[cfg(test)]
    pub(crate) async fn record_completion_cleanup_with_clock_for_test(
        &self,
        id: i64,
        generation: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        self.record_completion_cleanup_with(id, generation, clock)
            .await
    }

    async fn record_completion_cleanup_with(
        &self,
        id: i64,
        generation: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        let _intent = self.completion_lock(id)?.lock_owned().await;
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
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
            let changed = connection
                .execute(
                    "UPDATE intents SET cleanup_proven = 1 WHERE id = ?1 AND kind = 'launch' AND runner_completed = 1 AND worker_cleanup_proven = 1 AND cleanup_proven = 0 AND EXISTS (SELECT 1 FROM completion_cleanup AS c WHERE c.intent_id = intents.id AND c.claim_generation = ?2 AND c.lease_until > ?3)",
                    (id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 0 {
                return Ok(false);
            }
            let removed = connection
                .execute(
                    "DELETE FROM completion_cleanup WHERE intent_id = ?1 AND claim_generation = ?2 AND lease_until > ?3",
                    (id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if removed != 1 {
                return Err(HostError::Journal);
            }
            Ok(true)
        }
        .await;
        finish_transition(&connection, &result).await?;
        self.sync_after(result).await
    }

    /// Return whether cleanup proof was committed before archive-lease retirement.
    pub(crate) async fn completion_worker_cleanup_proven(
        &self,
        id: i64,
    ) -> Result<bool, HostError> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT worker_cleanup_proven FROM intents WHERE id = ?1 AND kind = 'launch' AND runner_completed = 1",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?;
        row.ok_or(HostError::Journal)?
            .get(0)
            .map_err(|_| HostError::Journal)
    }
}

//! Durable completion events for runner cleanup and capacity refill.

use crate::error::HostError;
use crate::journal::Journal;
use crate::reconcile::IntentRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CleanupClaim {
    /// Monotonic fencing token for this cleanup attempt.
    pub(crate) generation: i64,
    /// Bounded retry count used only to select a backoff delay.
    pub(crate) attempt: u32,
}

impl Journal {
    /// Commit one exact scale-set completion before its queue message can be acknowledged.
    ///
    /// The deterministic runner name, runner ID, set ID, and request ID must match the launch.
    /// An unrelated runner returns `None` and does not change the journal.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Ownership`] when an event conflicts with a local launch.
    pub(crate) async fn record_runner_completed(
        &self,
        set_id: i64,
        request_id: i64,
        runner_id: i64,
        runner_name: &str,
    ) -> Result<Option<i64>, HostError> {
        if set_id <= 0 || request_id < 0 || runner_id <= 0 {
            return Err(HostError::Ownership);
        }
        let Some(launch_id) = local_launch_id(runner_name) else {
            return Ok(None);
        };
        let expected_assignment = format!("{set_id}:{request_id}");
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = completion_row(&connection, launch_id, &expected_assignment, runner_id).await;
        let ended = if result.is_ok() {
            connection.execute("COMMIT", ()).await
        } else {
            connection.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        self.sync_after(result).await
    }

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
        now: i64,
        lease_until: i64,
    ) -> Result<Option<CleanupClaim>, HostError> {
        if id <= 0 || lease_until <= now {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = claim_cleanup_row(&connection, id, now, lease_until).await;
        let ended = if result.is_ok() {
            connection.execute("COMMIT", ()).await
        } else {
            connection.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        self.sync_after(result).await
    }

    /// Set durable retry time after one cleanup attempt did not prove release.
    pub(crate) async fn retry_completion_cleanup(
        &self,
        id: i64,
        generation: i64,
        retry_after: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 || retry_after < 0 {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        let changed = connection
            .execute(
                "UPDATE completion_cleanup SET retry_after = ?1, lease_until = 0 WHERE intent_id = ?2 AND claim_generation = ?3 AND lease_until > 0",
                (retry_after, id, generation),
            )
            .await
            .map_err(|_| HostError::Journal);
        let changed = self.sync_after(changed).await?;
        Ok(changed == 1)
    }

    /// Mark worker and official-runner cleanup as proven before archive-lease retirement.
    ///
    /// This marker lets restart recovery finish lease retirement after a crash without
    /// repeating Docker or GitHub cleanup.
    pub(crate) async fn mark_completion_worker_cleanup_proven(
        &self,
        id: i64,
        generation: i64,
        now: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 || now < 0 {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        let changed = connection
            .execute(
                "UPDATE intents SET worker_cleanup_proven = 1 WHERE id = ?1 AND kind = 'launch' AND runner_completed = 1 AND cleanup_proven = 0 AND EXISTS (SELECT 1 FROM completion_cleanup AS c WHERE c.intent_id = intents.id AND c.claim_generation = ?2 AND c.lease_until > ?3)",
                (id, generation, now),
            )
            .await
            .map_err(|_| HostError::Journal);
        let marked = self.sync_after(changed.map(|rows| rows == 1)).await?;
        Ok(marked)
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
        now: i64,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 || now < 0 {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
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
        let ended = if result.is_ok() {
            connection.execute("COMMIT", ()).await
        } else {
            connection.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
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

async fn claim_cleanup_row(
    connection: &turso::Connection,
    id: i64,
    now: i64,
    lease_until: i64,
) -> Result<Option<CleanupClaim>, HostError> {
    let mut rows = connection
        .query(
            "SELECT COALESCE(c.attempts, 0), COALESCE(c.claim_generation, 0), COALESCE(c.retry_after, 0), COALESCE(c.lease_until, 0) FROM intents AS i LEFT JOIN completion_cleanup AS c ON c.intent_id = i.id WHERE i.id = ?1 AND i.kind = 'launch' AND i.runner_completed = 1 AND i.cleanup_proven = 0",
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
    if retry_after > now || current_lease > now {
        return Ok(None);
    }
    if attempts < 0 || generation < 0 {
        return Err(HostError::Journal);
    }
    let next_generation = generation.checked_add(1).ok_or(HostError::Journal)?;
    let attempt = attempts.saturating_add(1).min(5);
    let changed = connection
        .execute(
            "INSERT INTO completion_cleanup (intent_id, attempts, claim_generation, retry_after, lease_until) VALUES (?1, 1, 1, 0, ?2) ON CONFLICT(intent_id) DO UPDATE SET attempts = min(attempts + 1, 5), claim_generation = claim_generation + 1, lease_until = ?2 WHERE retry_after <= ?3 AND lease_until <= ?3 AND claim_generation = ?4",
            (id, lease_until, now, generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Ok(None);
    }
    Ok(Some(CleanupClaim {
        generation: next_generation,
        attempt: u32::try_from(attempt).map_err(|_| HostError::Journal)?,
    }))
}

async fn completion_row(
    connection: &turso::Connection,
    launch_id: &str,
    expected_assignment: &str,
    runner_id: i64,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT id, assignment_key, github_runner_id, jit_requested, runner_completed FROM intents WHERE kind = 'launch' AND launch_id = ?1 AND cleanup_proven = 0 ORDER BY id LIMIT 2",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let assignment: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let recorded_runner: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
    let jit_requested: bool = row.get(3).map_err(|_| HostError::Journal)?;
    let completed: bool = row.get(4).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || assignment
            .as_deref()
            .is_some_and(|value| value != expected_assignment)
        || recorded_runner
            .as_deref()
            .is_some_and(|value| value != runner_id.to_string())
        || !jit_requested
    {
        return Err(HostError::Ownership);
    }
    if completed {
        return Ok(Some(id));
    }
    let changed = connection
        .execute(
            "UPDATE intents SET github_runner_id = COALESCE(github_runner_id, ?1), runner_completed = 1 WHERE id = ?2 AND cleanup_proven = 0 AND jit_requested = 1 AND (github_runner_id IS NULL OR github_runner_id = ?1)",
            (runner_id.to_string(), id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    one_row(changed)?;
    Ok(Some(id))
}

fn local_launch_id(runner_name: &str) -> Option<&str> {
    let launch_id = runner_name.strip_prefix('v')?;
    if launch_id.len() == 32
        && launch_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Some(launch_id)
    } else {
        None
    }
}

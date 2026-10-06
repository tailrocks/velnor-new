//! Transaction helpers for completion cleanup claims.

use std::time::{SystemTime, UNIX_EPOCH};

use super::super::CleanupClaim;
use crate::error::HostError;

pub(super) async fn claim_cleanup_row(
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

pub(super) async fn finish_transition<T>(
    connection: &turso::Connection,
    result: &Result<T, HostError>,
) -> Result<(), HostError> {
    let ended = if result.is_ok() {
        connection.execute("COMMIT", ()).await
    } else {
        connection.execute("ROLLBACK", ()).await
    };
    ended.map(|_| ()).map_err(|_| HostError::Journal)
}

pub(super) fn unix_seconds() -> Result<i64, HostError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| HostError::Journal)
}

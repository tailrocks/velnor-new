// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::claim::{JitConflictRecoveryClaim, validate_claim_in_transaction};
use crate::{
    error::HostError,
    journal::connection::{ConnectionJournal, storage::in_transaction},
};

/// GitHub publishes no completion bound for generatejitconfig. After 24 hours, a
/// newly created runner has a JIT config nobody holds, so it cannot come online
/// or run a job; GitHub removes offline ephemeral runners. This residual is accepted.
/// Claims without a recorded commit time use their first observation as the anchor.
const LATE_JIT_TOMBSTONE_RETENTION_SECONDS: i64 = 24 * 60 * 60;

pub(super) async fn active_tombstones(
    scope: &ConnectionJournal,
    now: i64,
) -> Result<Vec<(i64, String, i64)>, HostError> {
    crate::journal::connection::storage::read_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let mut rows = connection.query(
            "SELECT claim.reservation_id,claim.requested_name,claim.scale_set_id FROM jit_absent_after_deadline tombstone JOIN physical_attempt_jit_conflict_recovery_claims claim ON claim.reservation_id=tombstone.reservation_id AND claim.claim_generation=tombstone.claim_generation JOIN jit_dispatch_claim_times timing ON timing.intent_id=claim.intent_id WHERE claim.connection_id=?1 AND claim.binding_digest=?2 AND claim.requested_name='v'||claim.physical_nonce AND ?3<timing.commit_observed_at+?4 AND NOT EXISTS (SELECT 1 FROM jit_tombstone_completions completion WHERE completion.reservation_id=tombstone.reservation_id) ORDER BY claim.reservation_id",
            (scope.connection_id().as_str(), scope.binding_digest(), now, LATE_JIT_TOMBSTONE_RETENTION_SECONDS),
        ).await.map_err(|_| HostError::Journal)?;
        let mut tombstones = Vec::new();
        while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
            tombstones.push((row.get(0).map_err(|_| HostError::Journal)?, row.get(1).map_err(|_| HostError::Journal)?, row.get(2).map_err(|_| HostError::Journal)?));
        }
        Ok(tombstones)
    }).await
}

pub(super) async fn stamp(scope: &ConnectionJournal, intent_id: i64) -> Result<(), HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let mut rows = connection
            .query("PRAGMA user_version", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let version: i64 = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get(0)
            .map_err(|_| HostError::Journal)?;
        drop(rows);
        if version >= 49 {
            stamp_in_transaction(connection, intent_id).await?;
        }
        Ok(())
    })
    .await
}

async fn stamp_in_transaction(
    connection: &turso::Connection,
    intent_id: i64,
) -> Result<(), HostError> {
    connection.execute("UPDATE jit_dispatch_claim_times SET commit_observed_at=unixepoch()+1 WHERE intent_id=?1 AND commit_observed_at IS NULL", [intent_id]).await.map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn record(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
    now: i64,
) -> Result<bool, HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_claim_in_transaction(connection, scope, claim).await?;
        stamp_in_transaction(connection, claim.intent_id()).await?;
        let mut rows = connection.query("SELECT commit_observed_at FROM jit_dispatch_claim_times WHERE intent_id=?1", [claim.intent_id()]).await.map_err(|_| HostError::Journal)?;
        let commit_observed_at: i64 = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Identity)?.get(0).map_err(|_| HostError::Journal)?;
        if now < commit_observed_at.checked_add(60).ok_or(HostError::Identity)? {
            return Ok(false);
        }
        drop(rows);
        connection.execute("INSERT OR IGNORE INTO jit_absent_after_deadline(reservation_id,claim_generation,evaluated_at) VALUES (?1,?2,?3)", (claim.reservation_id(), claim.claim_generation(), now)).await.map_err(|_| HostError::Journal)?;
        Ok(true)
    }).await
}

pub(super) async fn complete(
    scope: &ConnectionJournal,
    reservation_id: i64,
    runner_id: i64,
) -> Result<(), HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let inserted = connection.execute("INSERT OR IGNORE INTO jit_tombstone_completions(reservation_id,runner_id) SELECT tombstone.reservation_id,?1 FROM jit_absent_after_deadline tombstone JOIN physical_attempt_jit_conflict_recovery_claims claim ON claim.reservation_id=tombstone.reservation_id AND claim.claim_generation=tombstone.claim_generation WHERE tombstone.reservation_id=?2 AND claim.connection_id=?3 AND claim.binding_digest=?4", (runner_id, reservation_id, scope.connection_id().as_str(), scope.binding_digest())).await.map_err(|_| HostError::Journal)?;
        if inserted != 1 { return Err(HostError::Journal); }
        Ok(())
    }).await
}

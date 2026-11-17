// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::claim::{JitConflictRecoveryClaim, validate_claim_in_transaction};
use crate::{
    error::HostError,
    journal::connection::{ConnectionJournal, storage::in_transaction},
};

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

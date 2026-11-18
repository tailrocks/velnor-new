// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{UnboundResourceKind, UnboundRetirementClaim, validate_claim_in_transaction};
use crate::journal::connection::storage::in_transaction;
use crate::{error::HostError, journal::ConnectionJournal};

impl ConnectionJournal {
    pub(crate) async fn anchor_bootstrap_create_clock(
        &self,
        reservation_id: i64,
        kind: UnboundResourceKind,
        role: &str,
    ) -> Result<(), HostError> {
        in_transaction(self.journal_ref(),async |connection| {
            self.validate_scope(connection).await?;
            #[cfg(test)]
            {
                let mut rows=connection.query("PRAGMA user_version",()).await.map_err(|_|HostError::Journal)?;
                if rows.next().await.map_err(|_|HostError::Journal)?.ok_or(HostError::Journal)?.get::<i64>(0).map_err(|_|HostError::Journal)?<51 {return Ok(())}
            }
            connection.execute("UPDATE bootstrap_create_claim_clocks SET claimed_at=unixepoch()+1 WHERE reservation_id=?1 AND resource_kind=?2 AND role=?3 AND claimed_at IS NULL AND EXISTS (SELECT 1 FROM capacity_reservations reservation WHERE reservation.id=?1 AND reservation.connection_id=?4)",(reservation_id,kind.as_str(),role,self.connection_id().as_str())).await.map_err(|_|HostError::Journal)?;
            Ok(())
        }).await
    }

    pub(crate) async fn unbound_creation_deadline_elapsed(
        &self,
        claim: &UnboundRetirementClaim,
        kind: UnboundResourceKind,
        role: &str,
        now: i64,
    ) -> Result<bool, HostError> {
        self.anchor_bootstrap_create_clock(claim.reservation_id, kind, role)
            .await?;
        in_transaction(self.journal_ref(),async |connection| {
            validate_claim_in_transaction(connection,claim).await?;
            let mut rows=connection.query("SELECT claimed_at FROM bootstrap_create_claim_clocks WHERE reservation_id=?1 AND resource_kind=?2 AND role=?3",(claim.reservation_id,kind.as_str(),role)).await.map_err(|_|HostError::Journal)?;
            let Some(row)=rows.next().await.map_err(|_|HostError::Journal)? else {return Ok(false)};
            let Some(claimed)=row.get::<Option<i64>>(0).map_err(|_|HostError::Journal)? else{return Ok(false)};
            Ok(now.checked_sub(claimed).is_some_and(|age|age>=60))
        }).await
    }

    pub(crate) async fn record_unbound_creation_absent_after_deadline(
        &self,
        claim: &UnboundRetirementClaim,
        kind: UnboundResourceKind,
        role: &str,
        name: &str,
        now: i64,
    ) -> Result<bool, HostError> {
        self.validate_claim_identity(claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_claim_in_transaction(connection,claim).await?;
            let mut rows=connection.query("SELECT claimed_at FROM bootstrap_create_claim_clocks WHERE reservation_id=?1 AND resource_kind=?2 AND role=?3 AND docker_name=?4",(claim.reservation_id,kind.as_str(),role,name)).await.map_err(|_|HostError::Journal)?;
            let Some(row)=rows.next().await.map_err(|_|HostError::Journal)? else {return Ok(false)};
            let claimed: i64=row.get(0).map_err(|_|HostError::Journal)?;
            if now.checked_sub(claimed).is_none_or(|age|age<60) {return Ok(false)}
            connection.execute("INSERT OR IGNORE INTO bootstrap_create_absences VALUES(?1,?2,?3,?4,?5)",(claim.reservation_id,kind.as_str(),role,name,now)).await.map_err(|_|HostError::Journal)?;
            Ok(true)
        }).await
    }
}

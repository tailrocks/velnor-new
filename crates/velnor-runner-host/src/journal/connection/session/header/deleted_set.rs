// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::ConnectionJournal;
use crate::journal::capacity_header::{HeaderAllocation, load, save};
use crate::journal::connection::storage::in_transaction;

use super::verify_allocation;

impl ConnectionJournal {
    pub(crate) async fn settle_header_of_deleted_set(
        &self,
        session_intent_id: i64,
        allocation: &HeaderAllocation,
    ) -> Result<(), HostError> {
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            verify_allocation(
                connection,
                self,
                scale_set_id,
                session_intent_id,
                allocation,
            )
            .await?;
            let mut rows = connection
                .query(
                    "SELECT 1 FROM connection_scale_set_deletes WHERE connection_id = ?1 AND scale_set_id = ?2 AND state = 'deleted'",
                    (self.connection_id().as_str(), scale_set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_none() {
                return Err(HostError::Journal);
            }
            drop(rows);
            let mut ledger = load(connection).await?;
            ledger
                .settle_zero_header_of_deleted_set(allocation)
                .map_err(|_| HostError::Journal)?;
            save(connection, &ledger).await
        })
        .await
    }
}

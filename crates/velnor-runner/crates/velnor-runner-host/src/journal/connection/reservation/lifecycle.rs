//! Reservation uncertainty and proof-based release.

use super::super::ConnectionJournal;
use super::super::storage::in_transaction;
use crate::error::HostError;
#[cfg(test)]
use crate::journal::sql::one_row;

impl ConnectionJournal {
    /// Keep a reservation charged after an uncertain remote result.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the reservation is missing or released.
    #[cfg(test)]
    pub(crate) async fn mark_uncertain(&self, reservation_id: i64) -> Result<(), HostError> {
        self.update_reservation_state(reservation_id, "uncertain")
            .await
    }

    /// Mark a reservation for recovery without releasing its charge.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the reservation is missing or released.
    #[cfg(test)]
    pub(crate) async fn mark_recovering(&self, reservation_id: i64) -> Result<(), HostError> {
        self.update_reservation_state(reservation_id, "recovering")
            .await
    }

    /// Record remote cleanup proof for one reservation.
    ///
    /// The charge ends only after local cleanup proof also exists.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the reservation belongs elsewhere.
    pub(crate) async fn record_remote_cleanup(&self, reservation_id: i64) -> Result<(), HostError> {
        self.record_cleanup_proof(reservation_id, true).await
    }

    /// Record local cleanup proof for one reservation.
    ///
    /// The charge ends only after remote cleanup proof also exists.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the reservation belongs elsewhere.
    pub(crate) async fn record_local_cleanup(&self, reservation_id: i64) -> Result<(), HostError> {
        self.record_cleanup_proof(reservation_id, false).await
    }
    #[cfg(test)]
    async fn update_reservation_state(
        &self,
        reservation_id: i64,
        state: &str,
    ) -> Result<(), HostError> {
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservations SET state = ?1 WHERE id = ?2 AND connection_id = ?3 AND state != 'released'",
                    (state, reservation_id, self.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            one_row(changed)
        })
        .await
    }

    async fn record_cleanup_proof(
        &self,
        reservation_id: i64,
        remote: bool,
    ) -> Result<(), HostError> {
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let column = if remote {
                "remote_cleanup_proven"
            } else {
                "local_cleanup_proven"
            };
            connection
                .execute(
                    &format!(
                        "UPDATE capacity_reservations SET {column} = 1 WHERE id = ?1 AND connection_id = ?2"
                    ),
                    (reservation_id, self.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut rows = connection
                .query(
                    "SELECT intent_id, state, remote_cleanup_proven, local_cleanup_proven, EXISTS (SELECT 1 FROM capacity_reservation_provisioning AS p WHERE p.reservation_id = capacity_reservations.id), COALESCE((SELECT storage_cleanup_proven FROM capacity_reservation_provisioning_state AS s WHERE s.reservation_id = capacity_reservations.id), 0) FROM capacity_reservations WHERE id = ?1 AND connection_id = ?2",
                    (reservation_id, self.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
            let intent_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
            let state: String = row.get(1).map_err(|_| HostError::Journal)?;
            let remote_proven: i64 = row.get(2).map_err(|_| HostError::Journal)?;
            let local_proven: i64 = row.get(3).map_err(|_| HostError::Journal)?;
            let has_storage_hold: i64 = row.get(4).map_err(|_| HostError::Journal)?;
            let storage_cleanup_proven: i64 = row.get(5).map_err(|_| HostError::Journal)?;
            if remote_proven == 1 && local_proven == 1 {
                crate::journal::request_lifecycle::physical::mark_runner_cleaned_after_cleanup(
                    connection,
                    self.connection_id.as_str(),
                    intent_id,
                )
                .await?;
            }
            let storage_ready = has_storage_hold == 0 || storage_cleanup_proven == 1;
            if remote_proven == 1
                && local_proven == 1
                && storage_ready
                && state != "released"
                && !super::provisioning::post_jit_retirement_claim_exists(connection, reservation_id)
                .await?
                && super::provisioning::release_ready(connection, reservation_id).await?
            {
                let changed = connection
                    .execute(
                        "UPDATE capacity_reservations SET state = 'released' WHERE id = ?1 AND connection_id = ?2 AND remote_cleanup_proven = 1 AND local_cleanup_proven = 1 AND state != 'released'",
                        (reservation_id, self.connection_id.as_str()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                if changed != 1 {
                    return Err(HostError::Journal);
                }
                connection
                    .execute(
                        "UPDATE connection_intents SET cleanup_proven = 1 WHERE id = ?1 AND connection_id = ?2 AND cleanup_proven = 0",
                        (intent_id, self.connection_id.as_str()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                crate::journal::request_lifecycle::physical::mark_runner_cleaned_after_cleanup(
                    connection,
                    self.connection_id.as_str(),
                    intent_id,
                )
                .await?;
            }
            Ok(())
        })
        .await
    }
}

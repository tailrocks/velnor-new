//! Reservation uncertainty and proof-based release.

use super::super::ConnectionJournal;
use super::super::storage::in_transaction;
use super::storage::reservation_state;
use crate::error::HostError;
use crate::journal::sql::one_row;

impl ConnectionJournal {
    /// Keep a reservation charged after an uncertain remote result.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the reservation is missing or released.
    pub(crate) async fn mark_uncertain(&self, reservation_id: i64) -> Result<(), HostError> {
        self.update_reservation_state(reservation_id, "uncertain")
            .await
    }

    /// Mark a reservation for recovery without releasing its charge.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the reservation is missing or released.
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
                        "UPDATE capacity_reservations SET {column} = 1, state = CASE WHEN remote_cleanup_proven = 1 OR ?1 = 1 THEN CASE WHEN local_cleanup_proven = 1 OR ?2 = 1 THEN 'released' ELSE state END ELSE state END WHERE id = ?3 AND connection_id = ?4 AND state != 'released'"
                    ),
                    (i64::from(remote), i64::from(!remote), reservation_id, self.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = reservation_state(connection, reservation_id, self.connection_id.as_str())
                .await?;
            let Some(state) = row else {
                return Err(HostError::Journal);
            };
            if state == "released" {
                connection
                    .execute(
                        "UPDATE connection_intents SET cleanup_proven = 1 WHERE id = (SELECT intent_id FROM capacity_reservations WHERE id = ?1 AND connection_id = ?2) AND connection_id = ?2",
                        (reservation_id, self.connection_id.as_str()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
            }
            Ok(())
        })
        .await
    }
}

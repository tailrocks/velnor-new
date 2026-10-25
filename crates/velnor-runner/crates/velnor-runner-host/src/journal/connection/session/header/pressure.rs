use std::time::Instant;

use crate::error::HostError;
use crate::journal::ConnectionJournal;
use crate::journal::capacity_header::{HeaderAllocation, HeaderPollAttempt, load, save};
use crate::journal::connection::storage::{has_ambiguous_legacy_work, in_transaction};
use crate::launch::FreshPressureSample;

impl ConnectionJournal {
    pub(crate) async fn begin_session_pressure_poll(
        &self,
        session_intent_id: i64,
        allocation: &HeaderAllocation,
        sample: Option<FreshPressureSample>,
    ) -> Result<HeaderPollAttempt, HostError> {
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = self.required_set_id(connection).await?;
            super::verify_allocation(
                connection,
                self,
                scale_set_id,
                session_intent_id,
                allocation,
            )
            .await?;
            if has_ambiguous_legacy_work(connection).await? {
                return Err(HostError::Journal);
            }
            let occupied = occupied_work_floor(connection, self.connection_id().as_str()).await?;
            let mut ledger = load(connection).await?;
            let attempt = ledger
                .begin_pressure_poll(allocation, occupied, sample, Instant::now())
                .map_err(|_| HostError::Journal)?;
            save(connection, &ledger).await?;
            Ok(attempt)
        })
        .await
    }
}

async fn occupied_work_floor(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<u32, HostError> {
    let reservations = scalar_count_with_connection(
        connection,
        "SELECT COUNT(*) FROM capacity_reservations WHERE connection_id = ?1 AND state != 'released'",
        connection_id,
    )
    .await?;
    let unreserved_launches = scalar_count_with_connection(
        connection,
        "SELECT COUNT(*) FROM connection_intents AS launch WHERE launch.connection_id = ?1 AND launch.kind = 'launch' AND launch.cleanup_proven = 0 AND NOT EXISTS (SELECT 1 FROM capacity_reservations AS reservation WHERE reservation.intent_id = launch.id AND reservation.state != 'released')",
        connection_id,
    )
    .await?;
    let occupied = reservations
        .checked_add(unreserved_launches)
        .ok_or(HostError::Journal)?;
    u32::try_from(occupied).map_err(|_| HostError::Journal)
}

async fn scalar_count_with_connection(
    connection: &turso::Connection,
    query: &str,
    connection_id: &str,
) -> Result<u64, HostError> {
    let mut rows = connection
        .query(query, [connection_id])
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    u64::try_from(count).map_err(|_| HostError::Journal)
}

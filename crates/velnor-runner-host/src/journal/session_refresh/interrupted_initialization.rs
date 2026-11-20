// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const INTERRUPTED: &str = "SELECT init.reservation_id, init.connection_id, init.operation_id FROM capacity_reservation_storage_initialization AS init JOIN capacity_reservations AS reservation ON reservation.id = init.reservation_id AND reservation.intent_id = init.operation_id AND reservation.connection_id = init.connection_id JOIN connection_intents AS intent ON intent.id = init.operation_id AND intent.connection_id = init.connection_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = init.reservation_id WHERE reservation.state = 'reserved' AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0 AND intent.kind = 'launch' AND intent.state = 'pending' AND intent.cleanup_proven = 0 AND init.abort_state = 'not_requested' AND state.bootstrap_start_state = 'not_requested' AND state.resources_bound = 0 AND state.jit_state = 'not_requested' AND state.publication_state = 'not_requested' AND state.delivery_state = 'not_requested' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS container WHERE container.reservation_id = init.reservation_id AND (container.create_state != 'unclaimed' OR container.start_state != 'not_requested')) ORDER BY init.reservation_id";

const RESOLVE_INITIALIZER: &str = "UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = CASE WHEN create_dispatch_state = 'dispatched' THEN 'outcome_unknown' ELSE create_dispatch_state END, create_state = CASE WHEN start_state = 'not_requested' THEN 'uncertain' ELSE create_state END, start_state = CASE WHEN start_state = 'requested' THEN 'uncertain' ELSE start_state END, result_state = CASE WHEN start_state = 'started' AND result_state = 'not_requested' THEN 'uncertain' ELSE result_state END WHERE reservation_id = ?1 AND connection_id = ?2 AND operation_id = ?3 AND abort_state = 'not_requested'";

const HOLD_RESERVATION: &str = "UPDATE capacity_reservations SET state = 'uncertain' WHERE id = ?1 AND connection_id = ?2 AND intent_id = ?3 AND state = 'reserved' AND remote_cleanup_proven = 0 AND local_cleanup_proven = 0";

const HOLD_INTENT: &str = "UPDATE connection_intents SET state = 'uncertain' WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0";

pub(super) async fn mark_interrupted_storage_initializations(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(INTERRUPTED, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut interrupted = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        interrupted.push((
            row.get::<i64>(0).map_err(|_| HostError::Journal)?,
            row.get::<String>(1).map_err(|_| HostError::Journal)?,
            row.get::<i64>(2).map_err(|_| HostError::Journal)?,
        ));
    }
    drop(rows);
    for (reservation_id, connection_id, operation_id) in interrupted {
        let identity = (reservation_id, connection_id.as_str(), operation_id);
        let changed = [
            connection.execute(RESOLVE_INITIALIZER, identity).await,
            connection.execute(HOLD_RESERVATION, identity).await,
            connection
                .execute(HOLD_INTENT, (operation_id, connection_id.as_str()))
                .await,
        ];
        if changed.iter().any(|changed| !matches!(changed, Ok(1))) {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

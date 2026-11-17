// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{HostError, Journal, in_transaction};

const ABANDON_RESERVATIONS: &str = "UPDATE capacity_reservations SET state = 'uncertain' WHERE (?1 IS NULL OR id = ?1) AND (?2 IS NULL OR connection_id = ?2) AND state = 'reserved' AND remote_cleanup_proven = 0 AND local_cleanup_proven = 0 AND EXISTS (SELECT 1 FROM connection_intents AS i JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = capacity_reservations.id WHERE i.id = capacity_reservations.intent_id AND i.connection_id = capacity_reservations.connection_id AND i.kind = 'launch' AND i.state = 'pending' AND i.cleanup_proven = 0 AND i.jit_requested = 0 AND {pair})";

const ABANDON_INTENTS: &str = "UPDATE connection_intents SET state = 'uncertain' WHERE kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND jit_requested = 0 AND EXISTS (SELECT 1 FROM capacity_reservations AS r JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id WHERE r.intent_id = connection_intents.id AND r.connection_id = connection_intents.connection_id AND (?1 IS NULL OR r.id = ?1) AND (?2 IS NULL OR r.connection_id = ?2) AND r.state = 'uncertain' AND r.remote_cleanup_proven = 0 AND r.local_cleanup_proven = 0 AND {pair})";

const SHAPE_V45: &str = "{s}.resources_bound = 0 AND {s}.jit_state = 'not_requested' AND {s}.publication_state = 'not_requested' AND {s}.delivery_state = 'not_requested' AND {s}.storage_cleanup_proven = 0 AND (({s}.bootstrap_start_state IN ('started', 'uncertain') AND (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS pair_container WHERE pair_container.reservation_id = {s}.reservation_id) = 2) OR ({s}.bootstrap_start_state = 'not_requested' AND ((SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS pair_container WHERE pair_container.reservation_id = {s}.reservation_id) = 0 OR ((SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS pair_container WHERE pair_container.reservation_id = {s}.reservation_id) = 2 AND (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS pair_container WHERE pair_container.reservation_id = {s}.reservation_id AND pair_container.create_state = 'unclaimed' AND pair_container.start_state = 'not_requested' AND pair_container.container_id IS NULL) = 2)) AND EXISTS (SELECT 1 FROM capacity_reservation_storage_initialization AS init WHERE init.reservation_id = {s}.reservation_id AND init.abort_state = 'absent')))";

pub(super) async fn require_version(
    connection: &turso::Connection,
    versions: &[i64],
) -> Result<(), HostError> {
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
    if !versions.contains(&version) {
        return Err(HostError::Journal);
    }
    Ok(())
}

impl Journal {
    pub(crate) async fn recover_after_restart_v46_v47_for_test(&self) -> Result<(), HostError> {
        in_transaction(self, async |connection| {
            require_version(connection, &[46, 47]).await?;
            recover(connection).await
        })
        .await
    }
}

async fn recover(connection: &turso::Connection) -> Result<(), HostError> {
    connection.execute("UPDATE connection_session_refreshes SET state = 'abandoned_uncertain' WHERE state = 'requested'", ()).await.map_err(|_| HostError::Journal)?;
    super::super::execution_authorization::recover_at_startup(connection).await?;
    super::super::schema::jit_interrupted_claims::recover_at_startup(connection).await?;
    super::orphan_recovery::mark_interrupted_ready_reservations(connection).await?;
    recover_unbound_pairs(connection).await?;
    super::interrupted_initialization::mark_interrupted_storage_initializations(connection).await?;
    super::super::doctor_probe::recover_at_startup(connection).await
}

async fn recover_unbound_pairs(connection: &turso::Connection) -> Result<(), HostError> {
    let shape = SHAPE_V45.replace("{s}", "s");
    for template in [ABANDON_RESERVATIONS, ABANDON_INTENTS] {
        connection
            .execute(
                &template.replace("{pair}", &shape),
                (Option::<i64>::None, Option::<&str>::None),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection.execute("UPDATE capacity_reservation_unbound_retirement_claims SET claim_generation = claim_generation + 1, lease_until = 1 WHERE lease_until > 1", ()).await.map_err(|_| HostError::Journal)?;
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

use super::{UNBOUND_PAIR, UnboundContainer, UnboundInitializer, UnboundVolume};

const ELIGIBLE: &str = "SELECT r.remote_cleanup_proven + r.local_cleanup_proven, i.jit_requested, (SELECT COUNT(*) FROM capacity_reservation_provisioning_state AS s WHERE s.reservation_id = r.id AND {pair}), (SELECT COUNT(*) FROM connection_runner_jits AS jit WHERE jit.intent_id = i.id), (SELECT COUNT(*) FROM connection_execution_proofs AS proof WHERE proof.intent_id = i.id) + (SELECT COUNT(*) FROM connection_execution_decisions AS decision WHERE decision.intent_id = i.id) + (SELECT COUNT(*) FROM execution_publication_operations AS publication WHERE publication.reservation_id = r.id), (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS b WHERE b.reservation_id = r.id AND b.create_state = 'created' AND b.container_id IS NOT NULL), (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS b WHERE b.reservation_id = r.id), (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS o WHERE o.reservation_id = r.id AND o.create_state IN ('requested', 'uncertain')), (SELECT COUNT(*) FROM capacity_reservation_pre_jit_cleanup_claims AS pre WHERE pre.reservation_id = r.id) + (SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_claims AS post WHERE post.reservation_id = r.id), (SELECT COUNT(*) FROM physical_attempt_current AS current WHERE current.reservation_id = r.id AND current.intent_id = i.id) FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id WHERE r.id = ?1 AND r.connection_id = ?2";

pub(super) async fn eligible(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            &ELIGIBLE.replace("{pair}", UNBOUND_PAIR),
            (reservation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let mut values = [0_i64; 10];
    for (index, value) in values.iter_mut().enumerate() {
        *value = row.get(index).map_err(|_| HostError::Journal)?;
    }
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(values == [0, 0, 1, 0, 0, 2, 2, 0, 0, 1])
}

pub(super) async fn current_claim(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Option<(i64, i64)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT claim_generation, lease_until FROM capacity_reservation_unbound_retirement_claims WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some((
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
    )))
}

pub(super) async fn containers(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Vec<UnboundContainer>, HostError> {
    let mut rows = connection
        .query(
            "SELECT role, container_name, container_id FROM capacity_reservation_bootstrap_containers WHERE reservation_id = ?1 AND create_state = 'created' AND container_id IS NOT NULL ORDER BY CASE role WHEN 'runner' THEN 0 ELSE 1 END",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut containers = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        containers.push(UnboundContainer {
            role: row.get(0).map_err(|_| HostError::Journal)?,
            name: row.get(1).map_err(|_| HostError::Journal)?,
            id: row.get(2).map_err(|_| HostError::Journal)?,
        });
    }
    Ok(containers)
}

pub(super) async fn volumes(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Vec<UnboundVolume>, HostError> {
    let mut rows = connection
        .query(
            "SELECT role, docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND create_state = 'created' ORDER BY role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut volumes = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        volumes.push(UnboundVolume {
            role: row.get(0).map_err(|_| HostError::Journal)?,
            name: row.get(1).map_err(|_| HostError::Journal)?,
        });
    }
    Ok(volumes)
}

pub(super) async fn initializer(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Option<UnboundInitializer>, HostError> {
    let mut rows = connection
        .query(
            "SELECT helper_name, COALESCE(helper_id, abort_helper_id), operation_key FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1 AND remove_state != 'removed' AND abort_state != 'absent'",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some(UnboundInitializer {
        name: row.get(0).map_err(|_| HostError::Journal)?,
        id: row.get(1).map_err(|_| HostError::Journal)?,
        operation_key: row.get(2).map_err(|_| HostError::Journal)?,
    }))
}

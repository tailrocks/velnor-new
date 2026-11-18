// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

use super::{UnboundContainer, UnboundInitializer, UnboundVolume};

pub(super) async fn eligible(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
) -> Result<bool, HostError> {
    super::ownership::owns_cleanup(connection, connection_id, reservation_id).await
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
            "SELECT resource.role, resource.container_name, COALESCE(resource.container_id, target.docker_id) FROM capacity_reservation_bootstrap_containers resource LEFT JOIN capacity_reservation_unbound_creation_targets target ON target.reservation_id=resource.reservation_id AND target.resource_kind='container' AND target.role=resource.role AND target.docker_name=resource.container_name WHERE resource.reservation_id = ?1 AND resource.create_state IN ('created', 'uncertain') ORDER BY CASE resource.role WHEN 'runner' THEN 0 ELSE 1 END",
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
            "SELECT resource.role, resource.docker_name, COALESCE(resource.docker_id, target.docker_id) FROM capacity_reservation_provisioning_objects resource LEFT JOIN capacity_reservation_unbound_creation_targets target ON target.reservation_id=resource.reservation_id AND target.resource_kind='volume' AND target.role=resource.role AND target.docker_name=resource.docker_name WHERE resource.reservation_id = ?1 AND resource.create_state IN ('created', 'uncertain') ORDER BY resource.role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut volumes = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        volumes.push(UnboundVolume {
            role: row.get(0).map_err(|_| HostError::Journal)?,
            name: row.get(1).map_err(|_| HostError::Journal)?,
            id: row.get(2).map_err(|_| HostError::Journal)?,
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

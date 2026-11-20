// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::{in_transaction, read_transaction};

use super::{UnboundContainer, UnboundInitializer, UnboundVolume};

const IDENTITY: &str = "reservation_id = ?1 AND connection_id = ?2 AND engine_id = ?3 AND docker_root_digest = ?4 AND operation_key = ?5 AND helper_name = ?6 AND COALESCE(helper_id, abort_helper_id, '') = ?7";

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

impl ConnectionJournal {
    pub(crate) async fn unretired_storage_initializer(
        &self,
        reservation_id: i64,
        engine_id: &str,
        docker_root_digest: &str,
    ) -> Result<Option<UnboundInitializer>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT helper_name, COALESCE(helper_id, abort_helper_id), operation_key FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1 AND connection_id = ?2 AND engine_id = ?3 AND docker_root_digest = ?4 AND remove_state != 'removed' AND abort_state != 'absent'",
                    (reservation_id, connection_id.as_str(), engine_id, docker_root_digest),
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
        })
        .await
    }

    pub(crate) async fn settle_removed_storage_initializer(
        &self,
        reservation_id: i64,
        engine_id: &str,
        docker_root_digest: &str,
        helper: &UnboundInitializer,
    ) -> Result<(), HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let helper_id = helper.id.as_deref().unwrap_or("");
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let identity = (reservation_id, connection_id.as_str(), engine_id, docker_root_digest, helper.operation_key.as_str(), helper.name.as_str(), helper_id);
            for sql in [
                "UPDATE capacity_reservation_storage_initialization SET remove_state = 'requested' WHERE {IDENTITY} AND ?7 != '' AND helper_id = ?7 AND remove_state = 'not_requested' AND result_state = 'confirmed' AND abort_state = 'not_requested' AND EXISTS (SELECT 1 FROM capacity_reservation_storage_handoffs AS handoff WHERE handoff.reservation_id = capacity_reservation_storage_initialization.reservation_id AND handoff.connection_id = capacity_reservation_storage_initialization.connection_id AND handoff.operation_id = capacity_reservation_storage_initialization.operation_id AND handoff.state = 'attached')",
                "UPDATE capacity_reservation_storage_initialization SET remove_state = 'removed' WHERE {IDENTITY} AND ?7 != '' AND helper_id = ?7 AND remove_state = 'requested' AND result_state = 'confirmed' AND abort_state = 'not_requested'",
            ] {
                connection
                    .execute(&sql.replace("{IDENTITY}", IDENTITY), identity)
                    .await
                    .map_err(|_| HostError::Journal)?;
            }
            let mut rows = connection
                .query(
                    &format!("SELECT 1 FROM capacity_reservation_storage_initialization WHERE {IDENTITY} AND remove_state = 'removed' AND abort_state = 'not_requested'"),
                    identity,
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }
}

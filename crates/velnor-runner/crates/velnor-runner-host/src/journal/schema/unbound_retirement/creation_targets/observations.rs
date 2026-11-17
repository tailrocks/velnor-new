// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows=connection.query("SELECT observation_json, observation_sha256, reservation_id, operation_id, resource_kind, role, docker_name, docker_id, connection_id, engine_id, docker_root_digest FROM capacity_reservation_unbound_creation_targets", ()).await.map_err(|_|HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let json: String = row.get(0).map_err(|_| HostError::Journal)?;
        let digest: String = row.get(1).map_err(|_| HostError::Journal)?;
        let identity = crate::journal::unbound_observation::validate(&json, &digest)?;
        if identity.reservation_id != row.get::<i64>(2).map_err(|_| HostError::Journal)?
            || identity.operation_id != row.get::<i64>(3).map_err(|_| HostError::Journal)?
        {
            return Err(HostError::Journal);
        }
        for (index, expected) in [
            identity.resource_kind,
            identity.role,
            identity.docker_name,
            identity.docker_id,
            identity.connection_id,
            identity.engine_id,
            identity.docker_root_digest,
        ]
        .into_iter()
        .enumerate()
        {
            if expected
                != row
                    .get::<String>(index + 4)
                    .map_err(|_| HostError::Journal)?
            {
                return Err(HostError::Journal);
            }
        }
    }
    Ok(())
}

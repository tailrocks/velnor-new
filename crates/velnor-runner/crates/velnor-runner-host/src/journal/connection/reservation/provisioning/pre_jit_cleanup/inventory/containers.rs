use crate::error::HostError;

use super::{PreJitBootstrapContainer, PreJitStorageObject};

pub(super) async fn read_bootstrap(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Vec<PreJitBootstrapContainer>, HostError> {
    let mut rows = connection
        .query(
            "SELECT role, container_name, container_id, create_state, start_state FROM capacity_reservation_bootstrap_containers WHERE reservation_id = ?1 ORDER BY role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        records.push(PreJitBootstrapContainer {
            role: row.get(0).map_err(|_| HostError::Journal)?,
            name: row.get(1).map_err(|_| HostError::Journal)?,
            id: row.get(2).map_err(|_| HostError::Journal)?,
            create_state: row.get(3).map_err(|_| HostError::Journal)?,
            start_state: row.get(4).map_err(|_| HostError::Journal)?,
        });
    }
    Ok(records)
}

pub(super) async fn read_objects(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Vec<PreJitStorageObject>, HostError> {
    let mut rows = connection
        .query(
            "SELECT role, docker_name, docker_id, create_state FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 ORDER BY role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        records.push(PreJitStorageObject {
            role: row.get(0).map_err(|_| HostError::Journal)?,
            name: row.get(1).map_err(|_| HostError::Journal)?,
            id: row.get(2).map_err(|_| HostError::Journal)?,
            create_state: row.get(3).map_err(|_| HostError::Journal)?,
        });
    }
    Ok(records)
}

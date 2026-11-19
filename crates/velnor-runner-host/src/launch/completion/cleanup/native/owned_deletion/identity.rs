// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::models::{ContainerInspectResponse, Volume};

use crate::{
    error::HostError,
    journal::{StorageCleanupIdentity, StorageRole},
};

pub(super) fn validate_container(
    container: &ContainerInspectResponse,
    id: &str,
    name: &str,
    role: &str,
    identity: &StorageCleanupIdentity,
    connection_id: &str,
) -> Result<(), HostError> {
    let labels = container
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .ok_or(HostError::Ownership)?;
    let expected = HashMap::from([
        ("io.velnor.managed", "true".to_owned()),
        (
            "io.velnor.reservation-id",
            identity.reservation_id().to_string(),
        ),
        (
            "io.velnor.operation-id",
            identity.operation_id().to_string(),
        ),
        ("io.velnor.connection-id", connection_id.to_owned()),
        ("io.velnor.role", role.to_owned()),
    ]);
    let expected_name = format!("/{name}");
    if container.id.as_deref() != Some(id)
        || container.name.as_deref() != Some(expected_name.as_str())
        || expected
            .iter()
            .any(|(key, value)| labels.get(*key) != Some(value))
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

pub(super) fn validate_volume(
    volume: &Volume,
    name: &str,
    role: StorageRole,
    identity: &StorageCleanupIdentity,
    connection_id: &str,
) -> Result<(), HostError> {
    let expected = HashMap::from([
        ("velnor.connection", connection_id.to_owned()),
        ("velnor.reservation", identity.reservation_id().to_string()),
        ("velnor.operation", identity.operation_id().to_string()),
        ("velnor.role", role.as_str().to_owned()),
    ]);
    if volume.name != name
        || volume.driver != "local"
        || volume.labels.len() != expected.len()
        || expected
            .iter()
            .any(|(key, value)| volume.labels.get(*key) != Some(value))
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

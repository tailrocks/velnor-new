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

#[cfg(test)]
mod tests {
    use super::{validate_container, validate_volume};
    use crate::{
        error::HostError,
        journal::request_lifecycle::tests::native_started_intake_fixture,
        worker::{WorkerContainerRole, WorkerIdentityNames},
    };

    #[tokio::test]
    async fn container_cleanup_requires_exact_id_name_and_labels() -> Result<(), HostError> {
        let fixture = native_started_intake_fixture().await?;
        let identity = fixture
            .scope
            .storage_cleanup_identity(fixture.reservation_id)
            .await?
            .ok_or(HostError::Ownership)?;
        let names = WorkerIdentityNames::from_private_volume(identity.private_volume())?;
        let name = names.container_name(WorkerContainerRole::Runner);
        let id = identity.runner_container_id();
        let container = serde_json::from_value(serde_json::json!({
            "Id": id,
            "Name": format!("/{name}"),
            "Config": {"Labels": {
                "io.velnor.managed": "true",
                "io.velnor.reservation-id": identity.reservation_id().to_string(),
                "io.velnor.operation-id": identity.operation_id().to_string(),
                "io.velnor.connection-id": fixture.scope.connection_id().as_str(),
                "io.velnor.role": "runner"
            }}
        }))
        .map_err(|_| HostError::Frame)?;

        validate_container(
            &container,
            id,
            &name,
            "runner",
            &identity,
            fixture.scope.connection_id().as_str(),
        )?;
        assert!(matches!(
            validate_container(
                &container,
                "foreign-container-id",
                &name,
                "runner",
                &identity,
                fixture.scope.connection_id().as_str(),
            ),
            Err(HostError::Ownership)
        ));
        assert!(matches!(
            validate_container(
                &container,
                id,
                "foreign-name",
                "runner",
                &identity,
                fixture.scope.connection_id().as_str(),
            ),
            Err(HostError::Ownership)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn volume_cleanup_requires_exact_name_driver_and_labels() -> Result<(), HostError> {
        let fixture = native_started_intake_fixture().await?;
        let identity = fixture
            .scope
            .storage_cleanup_identity(fixture.reservation_id)
            .await?
            .ok_or(HostError::Ownership)?;
        let (role, name) = identity.volumes().first().ok_or(HostError::Ownership)?;
        let volume = serde_json::from_value(serde_json::json!({
            "Name": name,
            "Driver": "local",
            "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"),
            "Options": {},
            "Scope": "local",
            "Labels": {
                "velnor.connection": fixture.scope.connection_id().as_str(),
                "velnor.reservation": identity.reservation_id().to_string(),
                "velnor.operation": identity.operation_id().to_string(),
                "velnor.role": role.as_str()
            }
        }))
        .map_err(|_| HostError::Frame)?;

        validate_volume(
            &volume,
            name,
            *role,
            &identity,
            fixture.scope.connection_id().as_str(),
        )?;
        assert!(matches!(
            validate_volume(
                &volume,
                "foreign-volume",
                *role,
                &identity,
                fixture.scope.connection_id().as_str(),
            ),
            Err(HostError::Ownership)
        ));
        let mut foreign_driver = volume.clone();
        foreign_driver.driver = "foreign".to_owned();
        assert!(matches!(
            validate_volume(
                &foreign_driver,
                name,
                *role,
                &identity,
                fixture.scope.connection_id().as_str(),
            ),
            Err(HostError::Ownership)
        ));
        Ok(())
    }
}

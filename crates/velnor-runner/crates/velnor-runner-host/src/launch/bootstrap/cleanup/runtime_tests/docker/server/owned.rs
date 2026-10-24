use super::*;

impl super::super::StatefulDocker {
    pub(crate) async fn expose_secondary_owned_identity(
        &self,
        connection_id: &str,
        identity: crate::journal::StorageCleanupIdentity,
    ) {
        self.state.lock().await.secondary_owned_identity =
            Some((connection_id.to_owned(), identity));
    }
}

pub(super) fn owned_container_response(
    reference: &str,
    state: &ServerState,
) -> Result<Option<(u16, Vec<u8>)>, HostError> {
    if state.owned_containers_present
        && let Some(response) = container_response_for_identity(
            reference,
            &state.identity,
            state.owned_connection_id.as_deref(),
            state.owned_mismatch,
            &state.removed_owned_containers,
            &state.stopped_owned_containers,
        )?
    {
        return Ok(Some(response));
    }
    if let Some((connection_id, identity)) = &state.secondary_owned_identity {
        return container_response_for_identity(
            reference,
            identity,
            Some(connection_id),
            OwnedContainerMismatch::None,
            &state.removed_secondary_resources,
            &HashSet::new(),
        );
    }
    Ok(None)
}

fn container_response_for_identity(
    reference: &str,
    identity: &crate::journal::StorageCleanupIdentity,
    connection_id: Option<&str>,
    mismatch: OwnedContainerMismatch,
    removed: &HashSet<String>,
    stopped: &HashSet<String>,
) -> Result<Option<(u16, Vec<u8>)>, HostError> {
    if removed.contains(reference) {
        return Ok(None);
    }
    let (id, role, container_role) = if reference == identity.runner_container_id() {
        (
            identity.runner_container_id(),
            "runner",
            crate::worker::WorkerContainerRole::Runner,
        )
    } else if reference == identity.dind_container_id() {
        (
            identity.dind_container_id(),
            "dind",
            crate::worker::WorkerContainerRole::Dind,
        )
    } else {
        return Ok(None);
    };
    let connection_id = connection_id.ok_or(HostError::Ownership)?;
    let names = crate::worker::WorkerIdentityNames::from_private_volume(identity.private_volume())?;
    let name = names.container_name(container_role);
    let observed_id = if matches!(mismatch, OwnedContainerMismatch::Id) {
        "foreign-container-id".to_owned()
    } else {
        id.to_owned()
    };
    let observed_name = if matches!(mismatch, OwnedContainerMismatch::Name) {
        format!("/{name}-foreign")
    } else {
        format!("/{name}")
    };
    let observed_connection = if matches!(mismatch, OwnedContainerMismatch::Scope) {
        "foreign-scope".to_owned()
    } else {
        connection_id.to_owned()
    };
    let reservation_id = if matches!(mismatch, OwnedContainerMismatch::Reservation) {
        "0".to_owned()
    } else {
        identity.reservation_id().to_string()
    };
    let operation_id = if matches!(mismatch, OwnedContainerMismatch::Operation) {
        "0".to_owned()
    } else {
        identity.operation_id().to_string()
    };
    let runner_home = (role == "dind")
        .then(|| {
            identity
                .volumes()
                .iter()
                .find(|(volume_role, _)| *volume_role == crate::journal::StorageRole::RunnerHome)
                .map(|(_, name)| name.as_str())
        })
        .flatten();
    let running = role == "dind" && !stopped.contains(id);
    let body = owned_container_body(&OwnedContainerBody {
        role,
        id: &observed_id,
        name: &observed_name,
        reservation_id: &reservation_id,
        operation_id: &operation_id,
        connection_id: &observed_connection,
        runner_home,
        running,
    });
    Ok(Some((
        200,
        serde_json::to_vec(&body).map_err(|_| HostError::Frame)?,
    )))
}

pub(super) fn volume_response(
    name: &str,
    state: &ServerState,
) -> Result<Option<(u16, Vec<u8>)>, HostError> {
    if state.owned_volumes.contains(name) {
        let connection_id = state
            .owned_connection_id
            .as_deref()
            .ok_or(HostError::Ownership)?;
        return volume_response_for_identity(
            name,
            &state.identity,
            connection_id,
            matches!(
                state.volume_connection_mismatch,
                super::super::OwnedContainerMismatch::ForeignVolumeConnection
            ),
        )
        .map(Some);
    }
    if let Some((connection_id, identity)) = &state.secondary_owned_identity
        && !state.removed_secondary_resources.contains(name)
        && identity
            .volumes()
            .iter()
            .any(|(_, candidate)| candidate == name)
    {
        return volume_response_for_identity(name, identity, connection_id, false).map(Some);
    }
    Ok(None)
}

pub(super) fn remove_secondary_volume(name: &str, state: &mut ServerState) -> bool {
    let is_owned = state
        .secondary_owned_identity
        .as_ref()
        .is_some_and(|(_, identity)| {
            identity
                .volumes()
                .iter()
                .any(|(_, candidate)| candidate == name)
        });
    if is_owned {
        return state.removed_secondary_resources.insert(name.to_owned());
    }
    false
}

fn volume_response_for_identity(
    name: &str,
    identity: &crate::journal::StorageCleanupIdentity,
    connection_id: &str,
    connection_mismatch: bool,
) -> Result<(u16, Vec<u8>), HostError> {
    let (role, _) = identity
        .volumes()
        .iter()
        .find(|(_, candidate)| candidate == name)
        .ok_or(HostError::Ownership)?;
    let connection_label = if connection_mismatch {
        "foreign-connection"
    } else {
        connection_id
    };
    let body = serde_json::json!({
        "Name": name,
        "Driver": "local",
        "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"),
        "Options": {},
        "Scope": "local",
        "Labels": {
            "velnor.connection": connection_label,
            "velnor.reservation": identity.reservation_id().to_string(),
            "velnor.operation": identity.operation_id().to_string(),
            "velnor.role": role.as_str()
        }
    });
    Ok((
        200,
        serde_json::to_vec(&body).map_err(|_| HostError::Frame)?,
    ))
}

struct OwnedContainerBody<'a> {
    role: &'a str,
    id: &'a str,
    name: &'a str,
    reservation_id: &'a str,
    operation_id: &'a str,
    connection_id: &'a str,
    runner_home: Option<&'a str>,
    running: bool,
}

fn owned_container_body(input: &OwnedContainerBody<'_>) -> serde_json::Value {
    let OwnedContainerBody {
        role,
        id,
        name,
        reservation_id,
        operation_id,
        connection_id,
        runner_home,
        running,
    } = input;
    let (status, exit_code, finished_at) = if *running {
        ("running", serde_json::Value::Null, "0001-01-01T00:00:00Z")
    } else {
        ("exited", serde_json::json!(0), "2026-01-01T00:01:00Z")
    };
    serde_json::json!({
        "Id": id,
        "Name": name,
        "Image": "sha256:0123456789abcdef",
        "RestartCount": 0,
        "State": {
            "Status": status,
            "Running": running,
            "Paused": false,
            "Restarting": false,
            "OOMKilled": false,
            "Dead": false,
            "ExitCode": exit_code,
            "StartedAt": "2026-01-01T00:00:00Z",
            "FinishedAt": finished_at
        },
        "Config": {
            "Labels": {
                "io.velnor.managed": "true",
                "io.velnor.reservation-id": reservation_id,
                "io.velnor.operation-id": operation_id,
                "io.velnor.connection-id": connection_id,
                "io.velnor.role": role
            },
            "Env": ["RUNNER_DIAGNOSTIC_CANARY=must-not-be-written"]
        },
        "Mounts": runner_home.map(|name| vec![serde_json::json!({
            "Type": "volume",
            "Name": name,
            "Destination": "/home/runner",
            "RW": true
        })]).unwrap_or_default()
    })
}

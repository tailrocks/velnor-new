use super::*;

pub(super) fn owned_container_response(
    reference: &str,
    state: &ServerState,
) -> Result<Option<(u16, Vec<u8>)>, HostError> {
    if !state.owned_containers_present || state.removed_owned_containers.contains(reference) {
        return Ok(None);
    }
    let (id, role, container_role) = if reference == state.identity.runner_container_id() {
        (
            state.identity.runner_container_id(),
            "runner",
            crate::worker::WorkerContainerRole::Runner,
        )
    } else if reference == state.identity.dind_container_id() {
        (
            state.identity.dind_container_id(),
            "dind",
            crate::worker::WorkerContainerRole::Dind,
        )
    } else {
        return Ok(None);
    };
    let connection_id = state
        .owned_connection_id
        .as_deref()
        .ok_or(HostError::Ownership)?;
    let names =
        crate::worker::WorkerIdentityNames::from_private_volume(state.identity.private_volume())?;
    let name = names.container_name(container_role);
    let mismatch = state.owned_mismatch;
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
        state.identity.reservation_id().to_string()
    };
    let operation_id = if matches!(mismatch, OwnedContainerMismatch::Operation) {
        "0".to_owned()
    } else {
        state.identity.operation_id().to_string()
    };
    let runner_home = (role == "dind")
        .then(|| {
            state
                .identity
                .volumes()
                .iter()
                .find(|(volume_role, _)| *volume_role == crate::journal::StorageRole::RunnerHome)
                .map(|(_, name)| name.as_str())
        })
        .flatten();
    let running = role == "dind" && !state.stopped_owned_containers.contains(id);
    let body = owned_container_body(
        role,
        &observed_id,
        &observed_name,
        &reservation_id,
        &operation_id,
        &observed_connection,
        runner_home,
        running,
    );
    Ok(Some((
        200,
        serde_json::to_vec(&body).map_err(|_| HostError::Frame)?,
    )))
}

fn owned_container_body(
    role: &str,
    id: &str,
    name: &str,
    reservation_id: &str,
    operation_id: &str,
    connection_id: &str,
    runner_home: Option<&str>,
    running: bool,
) -> serde_json::Value {
    let (status, exit_code, finished_at) = if running {
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

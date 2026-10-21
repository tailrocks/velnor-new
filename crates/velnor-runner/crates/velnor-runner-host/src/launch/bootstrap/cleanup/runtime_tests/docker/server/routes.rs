use super::*;
use http::Request;

pub(super) async fn route(
    request: &Request,
    state: &Arc<Mutex<ServerState>>,
    effects: &Arc<EffectCounts>,
) -> Result<(u16, Vec<u8>), HostError> {
    let path = request.path.split('?').next().unwrap_or(&request.path);
    let mut state = state.lock().await;
    if path.ends_with("/info") {
        return Ok(info_json(&state.identity));
    }
    if path.contains("/images/") && path.ends_with("/json") {
        return image_response(path, &state);
    }
    if path.contains("/volumes/") {
        let name = path.rsplit('/').next().unwrap_or_default();
        if request.method == "GET" && state.owned_volumes.contains(name) {
            return volume_response(name, &state);
        }
        if request.method == "DELETE" && state.owned_volumes.contains(name) {
            if state.volume_delete_failure.as_deref() == Some(name) {
                return Ok((500, message()));
            }
            state.owned_volumes.remove(name);
            return Ok((204, Vec::new()));
        }
        return Ok((404, message()));
    }
    if path.contains("/networks/") && request.method == "DELETE" {
        return Ok((204, Vec::new()));
    }
    if path.ends_with("/containers/create") && request.method == "POST" {
        return create_probe(request, &mut state, effects);
    }
    if path.ends_with("/start") {
        if let Some(probe) = state.probe.as_mut() {
            probe.phase = ProbePhase::Running;
        }
        return Ok((204, Vec::new()));
    }
    if path.ends_with("/wait") {
        if let Some(probe) = state.probe.as_mut() {
            probe.phase = ProbePhase::Exited;
        }
        return Ok((200, br#"{"StatusCode":0}"#.to_vec()));
    }
    if path.contains("/containers/") && path.ends_with("/logs") {
        return capacity_logs(&state);
    }
    if path.contains("/containers/") && path.ends_with("/json") {
        return container_response(path, &state);
    }
    if path.contains("/containers/") && request.method == "DELETE" {
        if path.contains(state.identity.runner_container_id())
            || path.contains(state.identity.dind_container_id())
        {
            effects.container_deletes.fetch_add(1, Ordering::Relaxed);
            if state.owned_containers_present {
                let id = if path.contains(state.identity.runner_container_id()) {
                    state.identity.runner_container_id().to_owned()
                } else {
                    state.identity.dind_container_id().to_owned()
                };
                state.removed_owned_containers.insert(id);
            }
        }
        if let Some(probe) = state.probe.as_mut() {
            probe.phase = ProbePhase::Removed;
        }
        return Ok((204, Vec::new()));
    }
    Ok((404, message()))
}

fn volume_response(name: &str, state: &ServerState) -> Result<(u16, Vec<u8>), HostError> {
    let (role, _) = state
        .identity
        .volumes()
        .iter()
        .find(|(_, candidate)| candidate == name)
        .ok_or(HostError::Ownership)?;
    let connection_id = state
        .owned_connection_id
        .as_deref()
        .ok_or(HostError::Ownership)?;
    let body = serde_json::json!({
        "Name": name,
        "Driver": "local",
        "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"),
        "Options": {},
        "Scope": "local",
        "Labels": {
            "velnor.connection": connection_id,
            "velnor.reservation": state.identity.reservation_id().to_string(),
            "velnor.operation": state.identity.operation_id().to_string(),
            "velnor.role": role.as_str()
        }
    });
    Ok((
        200,
        serde_json::to_vec(&body).map_err(|_| HostError::Frame)?,
    ))
}

fn create_probe(
    request: &Request,
    state: &mut ServerState,
    effects: &EffectCounts,
) -> Result<(u16, Vec<u8>), HostError> {
    let mut config: serde_json::Value =
        serde_json::from_slice(&request.body).map_err(|_| HostError::Frame)?;
    let host_config = config
        .as_object_mut()
        .and_then(|object| object.remove("HostConfig"))
        .unwrap_or(serde_json::Value::Null);
    let name = request
        .path
        .split("name=")
        .nth(1)
        .unwrap_or("cleanup-probe")
        .split('&')
        .next()
        .unwrap_or("cleanup-probe")
        .to_owned();
    let observer = is_observer(&config);
    if !observer {
        state.capacity_runs = state.capacity_runs.saturating_add(1);
    }
    let capacity_available = if state.capacity_runs == 1 {
        state.capacity_first
    } else {
        state.capacity_after_first
    };
    let probe = ProbeState {
        id: PROBE_ID.to_owned(),
        name,
        config,
        host_config,
        phase: ProbePhase::Created,
        request_body: Vec::new(),
        capacity_available,
    };
    if observer {
        effects.observer_runs.fetch_add(1, Ordering::Relaxed);
    } else {
        effects.capacity_runs.fetch_add(1, Ordering::Relaxed);
    }
    state.probe = Some(probe);
    Ok((
        201,
        serde_json::to_vec(&serde_json::json!({"Id": PROBE_ID, "Warnings": []}))
            .map_err(|_| HostError::Frame)?,
    ))
}

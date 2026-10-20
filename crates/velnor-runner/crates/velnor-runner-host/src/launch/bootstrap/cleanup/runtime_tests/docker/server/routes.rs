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
        }
        if let Some(probe) = state.probe.as_mut() {
            probe.phase = ProbePhase::Removed;
        }
        return Ok((204, Vec::new()));
    }
    Ok((404, message()))
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
        0
    } else {
        19 * 1024 * 1024 * 1024
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
    return Ok((
        201,
        serde_json::to_vec(&serde_json::json!({"Id": PROBE_ID, "Warnings": []}))
            .map_err(|_| HostError::Frame)?,
    ));
}

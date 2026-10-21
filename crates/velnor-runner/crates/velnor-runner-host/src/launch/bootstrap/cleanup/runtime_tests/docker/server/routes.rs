use super::*;
use http::Request;
use tar::{Builder, EntryType, Header};

pub(super) async fn route(
    request: &Request,
    state: &Arc<Mutex<ServerState>>,
    effects: &Arc<EffectCounts>,
) -> Result<(u16, Vec<u8>), HostError> {
    let path = request.path.split('?').next().unwrap_or(&request.path);
    let mut state = state.lock().await;
    if path.contains("/containers/") && path.ends_with("/exec") && request.method == "POST" {
        return create_runner_log_exec(path, &request.body, &mut state);
    }
    if path.contains("/exec/") && path.ends_with("/json") && request.method == "GET" {
        return inspect_runner_log_exec(path, &state);
    }
    if path.ends_with("/info") {
        return Ok(info_json(&state.identity));
    }
    if path.contains("/images/") && path.ends_with("/json") {
        return image_response(path, &state);
    }
    if path.contains("/volumes/") {
        return route_volume(path, &request.method, &mut state);
    }
    if path.contains("/networks/") && request.method == "DELETE" {
        return Ok((204, Vec::new()));
    }
    if path.ends_with("/containers/create") && request.method == "POST" {
        return create_probe(request, &mut state, effects);
    }
    if let Some(response) = route_probe_lifecycle(path, &mut state) {
        return Ok(response);
    }
    if path.contains("/containers/") && path.ends_with("/stop") && request.method == "POST" {
        let reference = path
            .split("/containers/")
            .nth(1)
            .unwrap_or_default()
            .split('/')
            .next()
            .unwrap_or_default();
        if reference == state.identity.runner_container_id()
            || reference == state.identity.dind_container_id()
        {
            state.stopped_owned_containers.insert(reference.to_owned());
            return Ok((204, Vec::new()));
        }
        return Ok((404, message()));
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

fn route_probe_lifecycle(path: &str, state: &mut ServerState) -> Option<(u16, Vec<u8>)> {
    if path.ends_with("/start") {
        if let Some(probe) = state.probe.as_mut() {
            probe.phase = ProbePhase::Running;
        }
        return Some((204, Vec::new()));
    }
    if path.ends_with("/wait") {
        if let Some(probe) = state.probe.as_mut() {
            probe.phase = ProbePhase::Exited;
        }
        return Some((200, br#"{"StatusCode":0}"#.to_vec()));
    }
    None
}

fn route_volume(
    path: &str,
    method: &str,
    state: &mut ServerState,
) -> Result<(u16, Vec<u8>), HostError> {
    let name = path.rsplit('/').next().unwrap_or_default();
    if method == "GET" && state.owned_volumes.contains(name) {
        return volume_response(name, state);
    }
    if method == "DELETE" && state.owned_volumes.contains(name) {
        if state.volume_delete_failure.as_deref() == Some(name) {
            return Ok((500, message()));
        }
        state.owned_volumes.remove(name);
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

fn create_runner_log_exec(
    path: &str,
    body: &[u8],
    state: &mut ServerState,
) -> Result<(u16, Vec<u8>), HostError> {
    if !path.contains(state.identity.dind_container_id()) || !state.owned_containers_present {
        return Ok((404, message()));
    }
    let request: serde_json::Value = serde_json::from_slice(body).map_err(|_| HostError::Frame)?;
    if request.get("User").and_then(serde_json::Value::as_str) != Some("1000:1000")
        || request.get("Tty").and_then(serde_json::Value::as_bool) != Some(false)
        || request
            .get("Cmd")
            .and_then(serde_json::Value::as_array)
            .is_none()
    {
        return Ok((400, message()));
    }
    state.exec_counter = state.exec_counter.saturating_add(1);
    let id = format!("{}{:02x}", "f".repeat(62), state.exec_counter);
    state.exec_state = Some(super::ExecState {
        id: id.clone(),
        started: false,
        exit_code: None,
    });
    Ok((
        201,
        serde_json::json!({"Id": id, "Warnings": []})
            .to_string()
            .into_bytes(),
    ))
}

pub(super) async fn start_runner_log_exec(
    request: &Request,
    state: &Arc<Mutex<ServerState>>,
) -> Result<Option<Vec<u8>>, HostError> {
    let path = request.path.split('?').next().unwrap_or(&request.path);
    if request.method != "POST" || !path.contains("/exec/") || !path.ends_with("/start") {
        return Ok(None);
    }
    let mut state = state.lock().await;
    let empty_logs = state.empty_runner_logs;
    let failed_exec = state.runner_log_exec_failures_remaining > 0;
    if failed_exec {
        state.runner_log_exec_failures_remaining -= 1;
    }
    let Some(exec) = state.exec_state.as_mut() else {
        return Ok(Some(message()));
    };
    let id = path
        .split("/exec/")
        .nth(1)
        .unwrap_or_default()
        .split('/')
        .next()
        .unwrap_or_default();
    if id != exec.id || exec.started {
        return Ok(Some(message()));
    }
    exec.started = true;
    exec.exit_code = Some(if failed_exec { 1 } else { 0 });
    let archive = if empty_logs {
        empty_root_archive()?
    } else {
        include_bytes!(
            "../../../../../completion/diagnostics/runner_logs/collect_tests/orb-volume-archive.tar"
        )
        .to_vec()
    };
    Ok(Some(archive))
}

fn empty_root_archive() -> Result<Vec<u8>, HostError> {
    let mut builder = Builder::new(Vec::new());
    let mut header = Header::new_ustar();
    header.set_entry_type(EntryType::Directory);
    header.set_size(0);
    header.set_mode(0o700);
    header.set_uid(1000);
    header.set_gid(1000);
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append_data(&mut header, ".", std::io::empty())
        .map_err(|_| HostError::Journal)?;
    builder.into_inner().map_err(|_| HostError::Journal)
}

fn inspect_runner_log_exec(path: &str, state: &ServerState) -> Result<(u16, Vec<u8>), HostError> {
    let id = path
        .split("/exec/")
        .nth(1)
        .unwrap_or_default()
        .split('/')
        .next()
        .unwrap_or_default();
    let Some(exec) = state.exec_state.as_ref().filter(|exec| exec.id == id) else {
        return Ok((404, message()));
    };
    let body = serde_json::json!({
        "ID": exec.id,
        "ContainerID": state.identity.dind_container_id(),
        "Running": exec.started && exec.exit_code.is_none(),
        "ExitCode": exec.exit_code
    });
    Ok((200, body.to_string().into_bytes()))
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

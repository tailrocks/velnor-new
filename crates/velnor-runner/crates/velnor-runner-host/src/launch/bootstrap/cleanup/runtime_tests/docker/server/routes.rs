use super::*;
use http::Request;
use tar::{Builder, EntryType, Header};

impl super::super::StatefulDocker {
    pub(crate) async fn fail_container_delete_once(&self, reference: &str) {
        self.state.lock().await.container_delete_failure_once = Some(reference.to_owned());
    }

    pub(crate) async fn container_delete_attempts(&self) -> Vec<(String, u16)> {
        self.state.lock().await.container_delete_attempts.clone()
    }

    pub(crate) async fn volume_delete_attempts(&self) -> Vec<(String, u16)> {
        self.state.lock().await.volume_delete_attempts.clone()
    }

    pub(crate) async fn expose_owned_volumes(&self, connection_id: &str) {
        let mut state = self.state.lock().await;
        state.owned_connection_id = Some(connection_id.to_owned());
        state.owned_volumes = state
            .identity
            .volumes()
            .iter()
            .map(|(_, name)| name.clone())
            .collect();
    }
}

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
        return Ok(inspect_runner_log_exec(path, &state));
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
        return Ok(route_container_stop(path, &mut state));
    }
    if path.contains("/containers/") && path.ends_with("/logs") {
        return capacity_logs(&state);
    }
    if path.contains("/containers/") && path.ends_with("/json") {
        return container_response(path, &state);
    }
    if path.contains("/containers/") && request.method == "DELETE" {
        return Ok(route_container_delete(path, &mut state, effects));
    }
    Ok((404, message()))
}

fn route_container_stop(path: &str, state: &mut ServerState) -> (u16, Vec<u8>) {
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
        return (204, Vec::new());
    }
    (404, message())
}

fn route_container_delete(
    path: &str,
    state: &mut ServerState,
    effects: &EffectCounts,
) -> (u16, Vec<u8>) {
    let reference = path
        .split("/containers/")
        .nth(1)
        .unwrap_or_default()
        .split('/')
        .next()
        .unwrap_or_default();
    let failed_once = state.container_delete_failure_once.as_deref() == Some(reference);
    let primary_id = [
        state.identity.runner_container_id(),
        state.identity.dind_container_id(),
    ]
    .into_iter()
    .find(|id| *id == reference)
    .map(str::to_owned);
    let secondary_id = state
        .secondary_owned_identity
        .as_ref()
        .and_then(|(_, identity)| {
            [identity.runner_container_id(), identity.dind_container_id()]
                .into_iter()
                .find(|id| *id == reference)
                .map(str::to_owned)
        });
    if primary_id.is_none() && secondary_id.is_none() {
        let current_probe = state
            .probe
            .as_ref()
            .is_some_and(|probe| probe.id.as_str() == reference);
        if current_probe {
            if failed_once {
                state.container_delete_failure_once = None;
            } else if let Some(probe) = state.probe.as_mut() {
                probe.phase = ProbePhase::Removed;
            }
            let status = if failed_once { 500 } else { 204 };
            state
                .container_delete_attempts
                .push((reference.to_owned(), status));
            return (status, if failed_once { message() } else { Vec::new() });
        }
        state
            .container_delete_attempts
            .push((reference.to_owned(), 404));
        return (404, message());
    }
    effects.container_deletes.fetch_add(1, Ordering::Relaxed);
    if !failed_once {
        if let Some(id) = primary_id.filter(|_| state.owned_containers_present) {
            state.removed_owned_containers.insert(id);
        } else if let Some(id) = secondary_id {
            state.removed_secondary_resources.insert(id);
        }
    }
    if failed_once {
        state.container_delete_failure_once = None;
    }
    let status = if failed_once { 500 } else { 204 };
    state
        .container_delete_attempts
        .push((reference.to_owned(), status));
    (status, if failed_once { message() } else { Vec::new() })
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
    if method == "GET" {
        if state.volume_deleted_before_inspection_failure.as_deref() == Some(name) {
            state.volume_inspection_failure_observed = true;
            return Ok((500, message()));
        }
        if let Some(response) = super::owned::volume_response(name, state)? {
            return Ok(response);
        }
    }
    if method == "DELETE" && state.owned_volumes.contains(name) {
        if state.volume_delete_failure.as_deref() == Some(name) {
            state.volume_delete_attempts.push((name.to_owned(), 500));
            return Ok((500, message()));
        }
        state.owned_volumes.remove(name);
        if state.volume_inspection_failure_after_delete.as_deref() == Some(name) {
            state.volume_deleted_before_inspection_failure = Some(name.to_owned());
        }
        state.volume_delete_attempts.push((name.to_owned(), 204));
        return Ok((204, Vec::new()));
    }
    if method == "DELETE" && super::owned::remove_secondary_volume(name, state) {
        state.volume_delete_attempts.push((name.to_owned(), 204));
        return Ok((204, Vec::new()));
    }
    if method == "DELETE" {
        state.volume_delete_attempts.push((name.to_owned(), 404));
    }
    Ok((404, message()))
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
    exec.exit_code = Some(i64::from(failed_exec));
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

fn inspect_runner_log_exec(path: &str, state: &ServerState) -> (u16, Vec<u8>) {
    let id = path
        .split("/exec/")
        .nth(1)
        .unwrap_or_default()
        .split('/')
        .next()
        .unwrap_or_default();
    let Some(exec) = state.exec_state.as_ref().filter(|exec| exec.id == id) else {
        return (404, message());
    };
    let body = serde_json::json!({
        "ID": exec.id,
        "ContainerID": state.identity.dind_container_id(),
        "Running": exec.started && exec.exit_code.is_none(),
        "ExitCode": exec.exit_code
    });
    (200, body.to_string().into_bytes())
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

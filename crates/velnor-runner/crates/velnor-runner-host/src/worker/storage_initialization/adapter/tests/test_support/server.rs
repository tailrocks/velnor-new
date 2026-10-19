use super::http::{CapturedRequest, normalized_route, read_http_request, write_http_response};
use super::*;

pub(super) struct ApiState {
    pub(super) volume_identity: super::VolumeIdentity,
    pub(super) home_volume: String,
    pub(super) work_volume: String,
    pub(super) metadata_volume: String,
    pub(super) image_id: String,
    pub(super) engine_id: String,
    pub(super) create_gate: Option<oneshot::Receiver<()>>,
    pub(super) create_seen: Option<oneshot::Sender<()>>,
    pub(super) create_completed: Option<oneshot::Sender<()>>,
    pub(super) response_gate: Option<oneshot::Receiver<()>>,
    pub(super) response_seen: Option<oneshot::Sender<()>>,
    pub(super) delete_response_gate: Option<oneshot::Receiver<()>>,
    pub(super) delete_response_seen: Option<oneshot::Sender<()>>,
    pub(super) inspect_response_gate: Option<oneshot::Receiver<()>>,
    pub(super) inspect_response_seen: Option<oneshot::Sender<()>>,
    pub(super) inspect_status: Option<u16>,
    pub(super) create_status: Option<u16>,
    pub(super) foreign_helper: bool,
    pub(super) remove_status: Option<u16>,
    pub(super) reported_helper_id: Option<String>,
    pub(super) start_status: Option<u16>,
    pub(super) create_config: Option<Value>,
    pub(super) helper_name: Option<String>,
    pub(super) lifecycle: ContainerLifecycle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ContainerLifecycle {
    Created,
    Started,
    Stopped,
    Removed,
}

pub(super) async fn serve_initialization_api(
    listener: UnixListener,
    state: ApiState,
    capture: Arc<Mutex<ApiCapture>>,
    mut stopped: oneshot::Receiver<()>,
) -> Result<(), String> {
    let mut state = state;
    let mut attach_tasks = Vec::new();
    loop {
        let Some(mut stream) = accept_request(&listener, &mut stopped, &mut attach_tasks).await?
        else {
            return Ok(());
        };
        let request = read_http_request(&mut stream).await?;
        let route = normalized_route(&request.target);
        capture
            .lock()
            .await
            .requests
            .push(format!("{} {}", request.method, route));
        if request.method == "POST" && route == "/containers/create" {
            state.helper_name = request
                .target
                .split_once("name=")
                .map(|(_, name)| name.split('&').next().unwrap_or(name).to_owned());
            capture.lock().await.create_name = state.helper_name.clone();
            if let Some(sender) = state.create_seen.take() {
                let _sent = sender.send(());
            }
            if let Some(gate) = state.create_gate.take() {
                let _released = gate.await;
            }
        }
        if request.method == "POST" && route.ends_with("/attach") {
            start_attach(stream, &state, &capture, &mut attach_tasks).await?;
            continue;
        }
        let (status, body) = route_request(&mut state, &request, &route)?;
        write_route_response(
            &mut state,
            &request.method,
            &route,
            stream,
            status,
            body,
            &mut attach_tasks,
        )
        .await?;
    }
}

async fn write_route_response(
    state: &mut ApiState,
    method: &str,
    route: &str,
    mut stream: UnixStream,
    status: u16,
    body: String,
    attach_tasks: &mut Vec<tokio::task::JoinHandle<Result<(), String>>>,
) -> Result<(), String> {
    let gate = if method == "GET" && route.starts_with("/containers/") && route.ends_with("/json") {
        state
            .inspect_response_gate
            .take()
            .map(|gate| (gate, &mut state.inspect_response_seen))
    } else if method == "DELETE" && route.starts_with("/containers/") {
        state
            .delete_response_gate
            .take()
            .map(|gate| (gate, &mut state.delete_response_seen))
    } else if method == "POST" && route == "/containers/create" {
        state
            .response_gate
            .take()
            .map(|gate| (gate, &mut state.response_seen))
    } else {
        None
    };
    if let Some((gate, seen)) = gate {
        if let Some(sender) = seen.take() {
            let _sent = sender.send(());
        }
        let completed = (method == "POST" && route == "/containers/create")
            .then(|| state.create_completed.take())
            .flatten();
        attach_tasks.push(tokio::spawn(async move {
            let _released = gate.await;
            let _response_result = write_http_response(&mut stream, status, body.as_bytes()).await;
            if let Some(sender) = completed {
                let _sent = sender.send(());
            }
            Ok(())
        }));
        return Ok(());
    }
    if method == "POST"
        && route == "/containers/create"
        && let Some(sender) = state.create_completed.take()
    {
        let _sent = sender.send(());
    }
    if write_http_response(&mut stream, status, body.as_bytes())
        .await
        .is_err()
        && !((method == "POST" && route == "/containers/create")
            || (method == "GET" && route.starts_with("/containers/") && route.ends_with("/json")))
    {
        return Err("Docker response write failed".to_owned());
    }
    Ok(())
}

async fn accept_request(
    listener: &UnixListener,
    stopped: &mut oneshot::Receiver<()>,
    attach_tasks: &mut Vec<tokio::task::JoinHandle<Result<(), String>>>,
) -> Result<Option<UnixStream>, String> {
    tokio::select! {
        _ = stopped => {
            for task in attach_tasks.drain(..) {
                task.await.map_err(|error| error.to_string())??;
            }
            Ok(None)
        }
        accepted = tokio::time::timeout(Duration::from_secs(10), listener.accept()) => {
            let (stream, _) = accepted.map_err(|_| "Docker request timed out".to_owned())?
                .map_err(|error| error.to_string())?;
            Ok(Some(stream))
        }
    }
}

async fn start_attach(
    mut stream: UnixStream,
    state: &ApiState,
    capture: &Arc<Mutex<ApiCapture>>,
    attach_tasks: &mut Vec<tokio::task::JoinHandle<Result<(), String>>>,
) -> Result<(), String> {
    stream
        .write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")
        .await
        .map_err(|error| error.to_string())?;
    let create = state
        .create_config
        .as_ref()
        .ok_or_else(|| "Attach preceded create".to_owned())?;
    let operation = create["Labels"]["velnor.operation"]
        .as_str()
        .ok_or_else(|| "Container operation label is absent".to_owned())?
        .to_owned();
    let capture = Arc::clone(capture);
    attach_tasks.push(tokio::spawn(async move {
        serve_attach_stream(stream, operation, capture).await
    }));
    Ok(())
}

pub(super) fn route_request(
    state: &mut ApiState,
    request: &CapturedRequest,
    route: &str,
) -> Result<(u16, String), String> {
    match (request.method.as_str(), route) {
        ("GET", "/info") => Ok((200, engine_response(&state.engine_id))),
        ("GET", path) if path.starts_with("/images/") => Ok((200, image_response(&state.image_id))),
        ("GET", path) if path.starts_with("/volumes/") => volume_response(state, path),
        ("POST", "/containers/create") => create_response(state, &request.body),
        ("GET", path) if path.starts_with("/containers/") && path.ends_with("/json") => {
            inspect_response(state)
        }
        ("POST", path) if path.ends_with("/start") => {
            state.lifecycle = ContainerLifecycle::Started;
            Ok((state.start_status.unwrap_or(204), String::new()))
        }
        ("POST", path) if path.ends_with("/stop") => {
            state.lifecycle = ContainerLifecycle::Stopped;
            Ok((204, String::new()))
        }
        ("POST", path) if path.ends_with("/wait") => {
            Ok((200, json!({"StatusCode": 0}).to_string()))
        }
        ("DELETE", path) if path.starts_with("/containers/") => {
            let status = state.remove_status.unwrap_or(204);
            if status == 204 {
                state.lifecycle = ContainerLifecycle::Removed;
            }
            Ok((status, String::new()))
        }
        _ => Err("Docker request is outside the initializer test".to_owned()),
    }
}

fn engine_response(engine_id: &str) -> String {
    json!({"ID": engine_id, "DockerRootDir": "/var/lib/docker", "Architecture": "aarch64", "OSType": "linux"}).to_string()
}

fn image_response(image_id: &str) -> String {
    json!({"Id": image_id, "Os": "linux", "Architecture": "amd64", "Config": {"Env": [IMAGE_PATH]}})
        .to_string()
}

fn volume_response(state: &ApiState, path: &str) -> Result<(u16, String), String> {
    let name = path.trim_start_matches("/volumes/");
    let (role, suffix) = if name == state.home_volume {
        ("runner-home", "-home")
    } else if name == state.work_volume {
        ("runner-work", "-work")
    } else if name == state.metadata_volume {
        ("engine-metadata", "-metadata")
    } else {
        return Err("Volume name is not owned".to_owned());
    };
    let worker = name
        .strip_suffix(suffix)
        .ok_or_else(|| "Volume name is invalid".to_owned())?;
    Ok((200, json!({"Name": name, "Driver": "local", "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"), "Labels": {"velnor.worker": worker, "velnor.connection": state.volume_identity.connection_id, "velnor.reservation": state.volume_identity.reservation_id.to_string(), "velnor.operation": state.volume_identity.operation_id.to_string(), "velnor.role": role}, "Options": {}, "Scope": "local"}).to_string()))
}

fn create_response(state: &mut ApiState, body: &[u8]) -> Result<(u16, String), String> {
    state.create_config = Some(serde_json::from_slice(body).map_err(|error| error.to_string())?);
    let operation = state
        .create_config
        .as_ref()
        .and_then(|config| config["Labels"]["velnor.operation"].as_str())
        .ok_or_else(|| "Container operation label is absent".to_owned())?;
    state.helper_name = Some(format!("velnor-init-{operation}"));
    Ok((
        state.create_status.unwrap_or(201),
        json!({"Id": HELPER_ID, "Warnings": []}).to_string(),
    ))
}

fn inspect_response(state: &ApiState) -> Result<(u16, String), String> {
    if state.create_config.is_none() || state.lifecycle == ContainerLifecycle::Removed {
        return Ok((
            state.inspect_status.unwrap_or(404),
            json!({"message": "No such container"}).to_string(),
        ));
    }
    let create = state
        .create_config
        .as_ref()
        .ok_or_else(|| "Inspect preceded create".to_owned())?;
    let mut config = create.clone();
    config["Env"] = json!([IMAGE_PATH]);
    if state.foreign_helper {
        config["Labels"]["velnor.operation"] = json!("foreign-operation");
    }
    config
        .as_object_mut()
        .ok_or_else(|| "Container config is invalid".to_owned())?
        .remove("NetworkDisabled");
    let mut host_config = create["HostConfig"].clone();
    host_config["CapAdd"] = json!(["CAP_FOWNER", "CAP_CHOWN"]);
    let name = state
        .helper_name
        .as_deref()
        .ok_or_else(|| "Container name is absent".to_owned())?;
    Ok((200, container_response(state, &config, &host_config, name)))
}

fn container_response(state: &ApiState, config: &Value, host_config: &Value, name: &str) -> String {
    json!({
        "Id": state.reported_helper_id.as_deref().unwrap_or(HELPER_ID), "Name": format!("/{name}"), "Image": state.image_id,
        "Mounts": [
            {"Type": "volume", "Name": state.home_volume, "Destination": "/velnor/init/home", "RW": true},
            {"Type": "volume", "Name": state.work_volume, "Destination": "/velnor/init/work", "RW": true},
            {"Type": "volume", "Name": state.metadata_volume, "Destination": "/velnor/init/metadata", "RW": true}
        ],
        "Config": config, "HostConfig": host_config,
        "State": {"Status": match state.lifecycle { ContainerLifecycle::Started => "running", ContainerLifecycle::Stopped => "exited", ContainerLifecycle::Created | ContainerLifecycle::Removed => "created" }, "StartedAt": if matches!(state.lifecycle, ContainerLifecycle::Started | ContainerLifecycle::Stopped) { "2026-10-19T00:00:00Z" } else { "0001-01-01T00:00:00Z" }},
        "NetworkSettings": {"SandboxID": "", "SandboxKey": "", "Ports": {}, "Networks": {"none": {
            "IPAMConfig": null, "Links": null, "Aliases": null, "DriverOpts": null, "GwPriority": 0,
            "NetworkID": "", "EndpointID": "", "Gateway": "", "IPAddress": "", "MacAddress": "",
            "IPPrefixLen": 0, "IPv6Gateway": "", "GlobalIPv6Address": "", "GlobalIPv6PrefixLen": 0, "DNSNames": null
        }}}
    }).to_string()
}

pub(super) async fn serve_attach_stream(
    mut stream: UnixStream,
    expected_operation: String,
    capture: Arc<Mutex<ApiCapture>>,
) -> Result<(), String> {
    let Some(payload) = read_bounded_line(&mut stream, 256).await? else {
        return Ok(());
    };
    if payload.is_empty() {
        return Ok(());
    }
    let input: Value = serde_json::from_slice(&payload).map_err(|error| error.to_string())?;
    let operation_key = input["operation_id"]
        .as_str()
        .ok_or_else(|| "Initializer request has no operation ID".to_owned())?;
    if operation_key != expected_operation {
        return Err("Initializer request operation does not match create".to_owned());
    }
    let mut result = format!(
            "{{\"schema\":2,\"operation_id\":\"{operation_key}\",\"home\":{{\"target\":\"/velnor/init/home\",\"mount_id\":31,\"parent_mount_id\":1,\"device_major\":0,\"device_minor\":48,\"inode\":61,\"uid\":1000,\"gid\":1000,\"mode\":493}},\"work\":{{\"target\":\"/velnor/init/work\",\"mount_id\":32,\"parent_mount_id\":1,\"device_major\":0,\"device_minor\":48,\"inode\":62,\"uid\":1000,\"gid\":1000,\"mode\":493}},\"metadata\":{{\"target\":\"/velnor/init/metadata\",\"mount_id\":33,\"parent_mount_id\":1,\"device_major\":0,\"device_minor\":48,\"inode\":63,\"uid\":0,\"gid\":0,\"mode\":365}},\"runtime_identity\":{{\"target\":\"/velnor/init/metadata/runtime-identity\",\"mount_id\":33,\"parent_mount_id\":33,\"device_major\":0,\"device_minor\":48,\"inode\":64,\"uid\":0,\"gid\":0,\"mode\":365}},\"job_facts\":{{\"target\":\"/velnor/init/metadata/job-facts\",\"mount_id\":33,\"parent_mount_id\":33,\"device_major\":0,\"device_minor\":48,\"inode\":65,\"uid\":0,\"gid\":0,\"mode\":365}},\"source_proof\":{{\"target\":\"/velnor/init/metadata/source-proof\",\"mount_id\":33,\"parent_mount_id\":33,\"device_major\":0,\"device_minor\":48,\"inode\":66,\"uid\":0,\"gid\":0,\"mode\":493}},\"control\":{{\"target\":\"/velnor/init/metadata/source-proof/control\",\"mount_id\":33,\"parent_mount_id\":33,\"device_major\":0,\"device_minor\":48,\"inode\":67,\"uid\":0,\"gid\":0,\"mode\":365}},\"inbox\":{{\"target\":\"/velnor/init/home/source-proof/inbox\",\"mount_id\":31,\"parent_mount_id\":31,\"device_major\":0,\"device_minor\":48,\"inode\":68,\"uid\":1000,\"gid\":1000,\"mode\":448}}}}"
        )
        .into_bytes();
    result.push(b'\n');
    let mut frame = Vec::with_capacity(8 + result.len());
    frame.extend_from_slice(&[1, 0, 0, 0]);
    let result_length = u32::try_from(result.len()).map_err(|error| error.to_string())?;
    frame.extend_from_slice(&result_length.to_be_bytes());
    frame.extend_from_slice(&result);
    stream
        .write_all(&frame)
        .await
        .map_err(|error| error.to_string())?;
    capture.lock().await.stdin = payload;
    let Some(acknowledgment) = read_bounded_line(&mut stream, 256).await? else {
        return Ok(());
    };
    let ack: Value = serde_json::from_slice(&acknowledgment).map_err(|error| error.to_string())?;
    if ack != json!({"operation_id": operation_key}) {
        return Err("Initializer acknowledgment does not match request".to_owned());
    }
    let mut trailing = [0_u8; 1];
    if stream
        .read(&mut trailing)
        .await
        .map_err(|error| error.to_string())?
        != 0
    {
        return Err("Initializer acknowledgment had trailing bytes".to_owned());
    }
    Ok(())
}

async fn read_bounded_line(
    stream: &mut UnixStream,
    limit: usize,
) -> Result<Option<Vec<u8>>, String> {
    let mut line = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let count = stream
            .read(&mut byte)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err("Truncated initializer protocol line".to_owned())
            };
        }
        line.push(byte[0]);
        if line.len() > limit {
            return Err("Initializer protocol line exceeded limit".to_owned());
        }
        if byte[0] == b'\n' {
            return Ok(Some(line));
        }
    }
}

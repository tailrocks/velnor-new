use super::*;
use http::Request;
use tokio::{
    io::AsyncWriteExt,
    net::{UnixListener, UnixStream},
};

#[path = "server/capacity.rs"]
mod capacity;
#[path = "server/http.rs"]
mod http;
#[path = "server/routes.rs"]
mod routes;

pub(super) async fn serve(
    listener: UnixListener,
    state: Arc<Mutex<ServerState>>,
    request_count: Arc<AtomicU64>,
    effects: Arc<EffectCounts>,
) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let state = Arc::clone(&state);
        let request_count = Arc::clone(&request_count);
        let effects = Arc::clone(&effects);
        tokio::spawn(async move {
            let _handled = handle(stream, state, request_count, effects).await;
        });
    }
}

async fn handle(
    mut stream: UnixStream,
    state: Arc<Mutex<ServerState>>,
    request_count: Arc<AtomicU64>,
    effects: Arc<EffectCounts>,
) -> Result<(), HostError> {
    let request = http::read_request(&mut stream).await?;
    request_count.fetch_add(1, Ordering::Relaxed);
    count_effect(&request, &effects);
    if request
        .path
        .ends_with("/attach?logs=false&stream=true&stdin=true&stdout=true&stderr=true")
    {
        stream
            .write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")
            .await
            .map_err(|_| HostError::Docker)?;
        let body = http::read_line(&mut stream).await?;
        let response = observer_output(&body, &state).await?;
        let mut frame = vec![1, 0, 0, 0];
        frame.extend_from_slice(&(response.len() as u32).to_be_bytes());
        frame.extend_from_slice(&response);
        stream
            .write_all(&frame)
            .await
            .map_err(|_| HostError::Docker)?;
        stream.shutdown().await.map_err(|_| HostError::Docker)?;
        return Ok(());
    }
    let (status, body) = routes::route(&request, &state, &effects).await?;
    http::write_response(&mut stream, status, &body).await
}

fn count_effect(request: &Request, effects: &EffectCounts) {
    if request.method != "DELETE" {
        return;
    }
    if request.path.contains("/volumes/") {
        effects.volume_deletes.fetch_add(1, Ordering::Relaxed);
    } else if request.path.contains("/networks/") {
        effects.network_deletes.fetch_add(1, Ordering::Relaxed);
    }
}

fn is_observer(config: &serde_json::Value) -> bool {
    config
        .get("Cmd")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|args| {
            args.iter()
                .any(|arg| arg.as_str() == Some("cleanup_observer"))
        })
}

fn capacity_logs(state: &ServerState) -> Result<(u16, Vec<u8>), HostError> {
    let probe = state.probe.as_ref().ok_or(HostError::Ownership)?;
    let mut output = serde_json::to_vec(&capacity::CapacityRecord::new(
        probe.capacity_available,
        state.capacity_total,
    ))
    .map_err(|_| HostError::Frame)?;
    output.push(b'\n');
    let mut frame = vec![1, 0, 0, 0];
    frame.extend_from_slice(&(output.len() as u32).to_be_bytes());
    frame.extend_from_slice(&output);
    Ok((200, frame))
}

fn info_json(identity: &StorageCleanupIdentity) -> (u16, Vec<u8>) {
    let body = serde_json::json!({
        "ID": identity.engine_id(),
        "DockerRootDir": ROOT,
        "OSType": "linux",
        "Architecture": "x86_64",
        "NCPU": 8,
        "MemTotal": 17_179_869_184_u64
    });
    (200, body.to_string().into_bytes())
}

fn message() -> Vec<u8> {
    br#"{"message":"missing"}"#.to_vec()
}

fn image_response(path: &str, state: &ServerState) -> Result<(u16, Vec<u8>), HostError> {
    let reference = path
        .trim_end_matches("/json")
        .split("/images/")
        .nth(1)
        .ok_or(HostError::Frame)?;
    let reference = reference.replace("%3A", ":").replace("%3a", ":");
    let image = state
        .images
        .iter()
        .find(|item| item.id == reference || item.tag == reference);
    let Some(image) = image else {
        return Ok((404, message()));
    };
    let body = serde_json::json!({"Id": image.id, "RepoTags": if image.tag.is_empty() { Vec::<String>::new() } else { vec![image.tag.clone()] }, "RepoDigests": [], "Os":"linux", "Architecture":"amd64", "Config": image.config, "Descriptor": image.descriptor});
    Ok((
        200,
        serde_json::to_vec(&body).map_err(|_| HostError::Frame)?,
    ))
}

fn container_response(path: &str, state: &ServerState) -> Result<(u16, Vec<u8>), HostError> {
    let Some(probe) = state.probe.as_ref() else {
        return Ok((404, message()));
    };
    if probe.phase == ProbePhase::Removed {
        return Ok((404, message()));
    }
    let reference = path
        .split("/containers/")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("");
    if reference != probe.id && reference != probe.name {
        return Ok((404, message()));
    }
    let config = &probe.config;
    let image = config
        .get("Image")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let Some(image_info) = state.images.iter().find(|item| item.id == image) else {
        return Ok((404, message()));
    };
    let inspected_config = merged_config(&image_info.config, config);
    let (status, running, exit_code, pid) = match probe.phase {
        ProbePhase::Created => ("created", false, 0, 0),
        ProbePhase::Running => ("running", true, 0, 1234),
        ProbePhase::Exited => ("exited", false, 0, 0),
        ProbePhase::Removed => return Ok((404, message())),
    };
    let mounts = inspect_mounts(&probe.host_config);
    let args = inspected_config
        .get("Cmd")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let body = serde_json::json!({"Id":probe.id,"Name":format!("/{}",probe.name),"Image":image,"RestartCount":0,"Path":"/velnor/resource-probe","Args":args,"Config":inspected_config,"HostConfig":probe.host_config,"Mounts":mounts,"State":{"Status":status,"Running":running,"ExitCode":exit_code,"OOMKilled":false,"Pid":pid}});
    Ok((
        200,
        serde_json::to_vec(&body).map_err(|_| HostError::Frame)?,
    ))
}

fn merged_config(image: &serde_json::Value, created: &serde_json::Value) -> serde_json::Value {
    let mut merged = image.as_object().cloned().unwrap_or_default();
    if let Some(created) = created.as_object() {
        for (key, value) in created {
            merged.insert(key.clone(), value.clone());
        }
    }
    serde_json::Value::Object(merged)
}

fn inspect_mounts(host_config: &serde_json::Value) -> serde_json::Value {
    let Some(mounts) = host_config
        .get("Mounts")
        .and_then(serde_json::Value::as_array)
    else {
        return serde_json::json!([]);
    };
    serde_json::Value::Array(
        mounts
            .iter()
            .filter_map(|mount| {
                let mut mount = mount.as_object()?.clone();
                let target = mount.remove("Target")?;
                let read_only = mount
                    .get("ReadOnly")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                mount.insert("Destination".to_owned(), target);
                mount.insert("RW".to_owned(), serde_json::Value::Bool(!read_only));
                Some(serde_json::Value::Object(mount))
            })
            .collect(),
    )
}

async fn observer_output(
    body: &[u8],
    state: &Arc<Mutex<ServerState>>,
) -> Result<Vec<u8>, HostError> {
    let request: velnor_resource_probe::cleanup_observer_wire::CleanupObserverRequest =
        serde_json::from_slice(body.strip_suffix(b"\n").ok_or(HostError::Frame)?)
            .map_err(|_| HostError::Frame)?;
    let mut state = state.lock().await;
    let probe = state.probe.as_mut().ok_or(HostError::Ownership)?;
    probe.request_body = body.to_vec();
    let binding = &request.binding;
    let record = velnor_resource_probe::cleanup_observer_wire::CleanupObserverRecord {
        schema_version: 1,
        operation_id: request.operation_id,
        binding_sha256: request.binding_sha256,
        guest_boot_id: Some(binding.expected_guest_boot_id.clone()),
        docker_root: Some(binding.docker_root.clone()),
        complete: true,
        owned_references_absent: true,
        issues: Vec::new(),
        task_count: 1,
        descriptor_count: 1,
        namespace_count: 2,
        owned_reference_count: 0,
        absent_volume_mountpoints: binding
            .volume_mountpoints
            .iter()
            .map(|volume| volume.mountpoint.clone())
            .collect(),
        absent_tmpfs_devices: binding.owned_tmpfs_devices.to_vec(),
        absent_mount_namespaces: [&binding.runner, &binding.dind]
            .into_iter()
            .map(
                |process| velnor_resource_probe::cleanup_observer_wire::NamespaceIdentity {
                    device: process.namespace_device,
                    inode: process.namespace_inode,
                },
            )
            .collect(),
        absent_cgroup_paths: Some(binding.cgroup_paths.clone()),
    };
    let mut output = serde_json::to_vec(&record).map_err(|_| HostError::Frame)?;
    output.push(b'\n');
    Ok(output)
}

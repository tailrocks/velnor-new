use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use bollard::Docker;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::Mutex,
};

use crate::{
    error::HostError,
    launch::bootstrap::PendingStorageCleanupWitness,
    worker::{WorkerContainerRole, WorkerIdentityNames},
};

const DOCKER_ROOT: &str = "/var/lib/docker";

#[derive(Clone, Copy)]
pub(super) enum DeleteMode {
    Reject,
    Keep,
}

pub(super) struct WorkerDocker {
    pub(super) docker: Docker,
    socket_path: PathBuf,
    pub(super) delete_count: Arc<AtomicU64>,
    pub(super) inspect_counts: [Arc<AtomicU64>; 2],
}

struct WorkerState {
    ids: [String; 2],
    names: [String; 2],
    labels: [serde_json::Value; 2],
    engine_id: String,
    running: [bool; 2],
    removed: [bool; 2],
    delete_mode: DeleteMode,
}

impl WorkerDocker {
    pub(super) fn open(
        pending: &PendingStorageCleanupWitness,
        connection_id: &str,
        delete_mode: DeleteMode,
    ) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-retirement-delete-{}-{id}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|_| HostError::Path)?;
        let identity = pending.identity();
        let names = WorkerIdentityNames::from_private_volume(identity.private_volume())?;
        let state = Arc::new(Mutex::new(WorkerState {
            ids: [
                identity.runner_container_id().to_owned(),
                identity.dind_container_id().to_owned(),
            ],
            names: [
                names.container_name(WorkerContainerRole::Runner),
                names.container_name(WorkerContainerRole::Dind),
            ],
            labels: [
                labels_json(identity, connection_id, "runner"),
                labels_json(identity, connection_id, "dind"),
            ],
            engine_id: identity.engine_id().to_owned(),
            running: [true, true],
            removed: [false, false],
            delete_mode,
        }));
        let delete_count = Arc::new(AtomicU64::new(0));
        let inspect_counts = [Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0))];
        tokio::spawn(serve(
            listener,
            Arc::clone(&state),
            Arc::clone(&delete_count),
            inspect_counts.clone(),
        ));
        let docker = Docker::connect_with_unix(
            path.to_str().ok_or(HostError::Path)?,
            120,
            bollard::API_DEFAULT_VERSION,
        )
        .map_err(|_| HostError::Docker)?;
        Ok(Self {
            docker,
            socket_path: path,
            delete_count,
            inspect_counts,
        })
    }
}

fn labels_json(
    identity: &crate::journal::StorageCleanupIdentity,
    connection_id: &str,
    role: &str,
) -> serde_json::Value {
    serde_json::json!({
        "io.velnor.managed": "true",
        "io.velnor.reservation-id": identity.reservation_id().to_string(),
        "io.velnor.operation-id": identity.operation_id().to_string(),
        "io.velnor.connection-id": connection_id,
        "io.velnor.role": role
    })
}

impl Drop for WorkerDocker {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_file(&self.socket_path);
    }
}

async fn serve(
    listener: UnixListener,
    state: Arc<Mutex<WorkerState>>,
    delete_count: Arc<AtomicU64>,
    inspect_counts: [Arc<AtomicU64>; 2],
) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let state = Arc::clone(&state);
        let delete_count = Arc::clone(&delete_count);
        let inspect_counts = inspect_counts.clone();
        tokio::spawn(async move {
            let _ignored = handle(stream, state, delete_count, inspect_counts).await;
        });
    }
}

async fn handle(
    mut stream: UnixStream,
    state: Arc<Mutex<WorkerState>>,
    delete_count: Arc<AtomicU64>,
    inspect_counts: [Arc<AtomicU64>; 2],
) -> Result<(), HostError> {
    let (method, path) = read_request(&mut stream).await?;
    let (status, body) = route(&method, &path, &state, &delete_count, &inspect_counts).await?;
    let reason = if status == 200 {
        "OK"
    } else if status == 204 {
        "No Content"
    } else if status == 500 {
        "Internal Server Error"
    } else {
        "Not Found"
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|_| HostError::Docker)?;
    stream
        .write_all(&body)
        .await
        .map_err(|_| HostError::Docker)?;
    Ok(())
}

async fn read_request(stream: &mut UnixStream) -> Result<(String, String), HostError> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| HostError::Docker)?;
        if read == 0 {
            return Err(HostError::Docker);
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let first = std::str::from_utf8(&bytes).map_err(|_| HostError::Frame)?;
    let mut parts = first
        .lines()
        .next()
        .ok_or(HostError::Frame)?
        .split_whitespace();
    let method = parts.next().ok_or(HostError::Frame)?.to_owned();
    let path = parts.next().ok_or(HostError::Frame)?.to_owned();
    Ok((method, path))
}

async fn route(
    method: &str,
    path: &str,
    state: &Arc<Mutex<WorkerState>>,
    delete_count: &AtomicU64,
    inspect_counts: &[Arc<AtomicU64>; 2],
) -> Result<(u16, Vec<u8>), HostError> {
    let mut state = state.lock().await;
    if path.ends_with("/info") {
        return Ok((200, info_json(&state)?.into_bytes()));
    }
    for (index, inspect_count) in inspect_counts.iter().enumerate() {
        if path.contains(&format!("/containers/{}/json", state.ids[index])) {
            inspect_count.fetch_add(1, Ordering::Relaxed);
            if state.removed[index] {
                return Ok((404, b"{}".to_vec()));
            }
            return Ok((200, inspect_json(&state, index)?.into_bytes()));
        }
        if path.contains(&format!("/containers/{}/stop", state.ids[index])) {
            state.running[index] = false;
            return Ok((204, Vec::new()));
        }
        if method == "DELETE" && path.contains(&format!("/containers/{}", state.ids[index])) {
            delete_count.fetch_add(1, Ordering::Relaxed);
            return Ok(delete_response(&state));
        }
    }
    Ok((404, b"{}".to_vec()))
}

fn delete_response(state: &WorkerState) -> (u16, Vec<u8>) {
    match state.delete_mode {
        DeleteMode::Reject => (500, b"{}".to_vec()),
        DeleteMode::Keep => (204, Vec::new()),
    }
}

fn info_json(state: &WorkerState) -> Result<String, HostError> {
    let value = serde_json::json!({
        "ID": state.engine_id,
        "DockerRootDir": DOCKER_ROOT,
        "OSType": "linux",
        "Architecture": "x86_64",
        "NCPU": 8,
        "MemTotal": 17_179_869_184_u64
    });
    serde_json::to_string(&value).map_err(|_| HostError::Frame)
}

fn inspect_json(state: &WorkerState, index: usize) -> Result<String, HostError> {
    let value = serde_json::json!({
        "Id": state.ids[index],
        "Name": format!("/{}", state.names[index]),
        "State": {"Running": state.running[index]},
        "Config": {"Labels": state.labels[index]}
    });
    serde_json::to_string(&value).map_err(|_| HostError::Frame)
}

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
    launch::{SelectedEngineContext, bootstrap::cleanup_runtime_fixture},
};

#[path = "network_id/reappearing_id.rs"]
mod reappearing_id;
#[path = "network_id/state_assertions.rs"]
mod state_assertions;
pub(super) use state_assertions::assert_network_phase;

const DOCKER_ROOT: &str = "/var/lib/docker";

#[test]
fn retirement_removes_saved_id_when_name_lookup_is_absent() -> Result<(), HostError> {
    run_on_production_stack(retirement_removes_saved_id_when_name_lookup_is_absent_inner)
}

async fn retirement_removes_saved_id_when_name_lookup_is_absent_inner() -> Result<(), HostError> {
    let (fixture, claim, selected, name, saved_id, labels) = prepare_case().await?;
    let docker = TestDocker::open(selected.engine_id(), &name, &saved_id, &labels, false, None)?;

    crate::launch::bootstrap::remove_owned_network_for_retirement(
        &docker.docker,
        &fixture.scope,
        &selected,
        fixture.reservation_id,
        &claim,
    )
    .await?;

    assert_eq!(
        docker.deleted_id().await.as_deref(),
        Some(saved_id.as_str())
    );
    assert!(!docker.network_is_present().await);
    assert_remove_state(&fixture, "removed").await
}

#[test]
fn retirement_does_not_delete_foreign_network_reusing_name() -> Result<(), HostError> {
    run_on_production_stack(retirement_does_not_delete_foreign_network_reusing_name_inner)
}

async fn retirement_does_not_delete_foreign_network_reusing_name_inner() -> Result<(), HostError> {
    let (fixture, claim, selected, name, saved_id, labels) = prepare_case().await?;
    let foreign_id = different_id(&saved_id);
    let docker = TestDocker::open(
        selected.engine_id(),
        &name,
        &saved_id,
        &labels,
        true,
        Some(&foreign_id),
    )?;

    assert!(
        crate::launch::bootstrap::remove_owned_network_for_retirement(
            &docker.docker,
            &fixture.scope,
            &selected,
            fixture.reservation_id,
            &claim,
        )
        .await
        .is_err()
    );

    assert_eq!(docker.deleted_id().await, None);
    assert!(docker.network_is_present().await);
    assert_remove_state(&fixture, "uncertain").await
}

fn run_on_production_stack<F, Fut>(test: F) -> Result<(), HostError>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), HostError>> + Send + 'static,
{
    const STACK_BYTES: usize = 8 * 1024 * 1024;
    std::thread::Builder::new()
        .name("retirement-network-test".to_owned())
        .stack_size(STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| HostError::Journal)?;
            runtime.block_on(test())
        })
        .map_err(|_| HostError::Journal)?
        .join()
        .map_err(|_| HostError::Journal)?
}

async fn prepare_case() -> Result<
    (
        crate::launch::bootstrap::CleanupRuntimeFixture,
        super::super::claim::PostJitRetirementClaim,
        SelectedEngineContext,
        String,
        String,
        std::collections::BTreeMap<String, String>,
    ),
    HostError,
> {
    let fixture = cleanup_runtime_fixture().await?;
    let selected = super::selected_engine(&fixture.scope, fixture.reservation_id).await?;
    let claim = super::prepare_local_remote(&fixture).await?;
    fixture
        .scope
        .record_retirement_storage_phase(&claim)
        .await?;
    let identity = fixture
        .scope
        .storage_network_recovery_identity(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let saved_id = identity.network_id().ok_or(HostError::Identity)?.to_owned();
    Ok((
        fixture,
        claim,
        selected,
        identity.network_name().to_owned(),
        saved_id,
        identity.labels().clone(),
    ))
}

async fn assert_remove_state(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    expected: &str,
) -> Result<(), HostError> {
    super::read_transaction(&fixture.journal, async |connection| {
        let mut rows = connection
            .query(
                "SELECT remove_state FROM capacity_reservation_networks WHERE reservation_id=?1 AND connection_id=?2",
                (fixture.reservation_id, fixture.scope.connection_id().as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        let state: String = row.get(0).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() || state != expected {
            return Err(HostError::Identity);
        }
        Ok(())
    })
    .await
}

fn different_id(value: &str) -> String {
    let mut chars = value.chars();
    let first = chars.next().unwrap_or('a');
    let replacement = if first == 'a' { 'b' } else { 'a' };
    format!("{replacement}{}", chars.as_str())
}

struct TestDocker {
    docker: Docker,
    state: Arc<Mutex<NetworkState>>,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl TestDocker {
    fn open(
        engine_id: &str,
        name: &str,
        saved_id: &str,
        labels: &std::collections::BTreeMap<String, String>,
        name_visible: bool,
        inspected_id: Option<&str>,
    ) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let socket_id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-retirement-network-{}-{socket_id}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|_| HostError::Path)?;
        let state = Arc::new(Mutex::new(NetworkState {
            engine_id: engine_id.to_owned(),
            name: name.to_owned(),
            saved_id: saved_id.to_owned(),
            inspected_id: inspected_id.unwrap_or(saved_id).to_owned(),
            labels: labels.clone(),
            name_visible,
            present: true,
            deleted_id: None,
            saved_id_inspections: 0,
            reappear_on_saved_id_inspection: None,
        }));
        let task_state = Arc::clone(&state);
        let task = tokio::spawn(async move { serve(listener, task_state).await });
        let socket = path.to_str().ok_or(HostError::Path)?;
        let docker = Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|_| HostError::Docker)?;
        Ok(Self {
            docker,
            state,
            path,
            task: Some(task),
        })
    }

    async fn deleted_id(&self) -> Option<String> {
        self.state.lock().await.deleted_id.clone()
    }

    async fn network_is_present(&self) -> bool {
        self.state.lock().await.present
    }

    async fn reappear_on_second_saved_id_inspection(&self) {
        let mut state = self.state.lock().await;
        state.present = false;
        state.reappear_on_saved_id_inspection = Some(2);
    }
}

impl Drop for TestDocker {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let _removed = std::fs::remove_file(&self.path).err();
    }
}

struct NetworkState {
    engine_id: String,
    name: String,
    saved_id: String,
    inspected_id: String,
    labels: std::collections::BTreeMap<String, String>,
    name_visible: bool,
    present: bool,
    deleted_id: Option<String>,
    saved_id_inspections: u8,
    reappear_on_saved_id_inspection: Option<u8>,
}

async fn serve(listener: UnixListener, state: Arc<Mutex<NetworkState>>) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let _served = serve_one(stream, state).await;
        });
    }
}

async fn serve_one(
    mut stream: UnixStream,
    state: Arc<Mutex<NetworkState>>,
) -> Result<(), HostError> {
    let (method, path) = read_request(&mut stream).await?;
    let (status, body) = route(&method, &path, &state).await?;
    let phrase = match status {
        200 => "OK",
        204 => "No Content",
        _ => "Not Found",
    };
    let header = format!(
        "HTTP/1.1 {status} {phrase}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .await
        .map_err(|_| HostError::Docker)?;
    stream.write_all(&body).await.map_err(|_| HostError::Docker)
}

async fn read_request(stream: &mut UnixStream) -> Result<(String, String), HostError> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 2048];
    loop {
        let read = stream
            .read(&mut buffer)
            .await
            .map_err(|_| HostError::Docker)?;
        if read == 0 || bytes.len() > 32 * 1024 {
            return Err(HostError::Frame);
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            break;
        }
    }
    let end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or(HostError::Frame)?;
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| HostError::Frame)?;
    let mut first = header
        .lines()
        .next()
        .ok_or(HostError::Frame)?
        .split_whitespace();
    Ok((
        first.next().ok_or(HostError::Frame)?.to_owned(),
        first.next().ok_or(HostError::Frame)?.to_owned(),
    ))
}

async fn route(
    method: &str,
    path: &str,
    state: &Arc<Mutex<NetworkState>>,
) -> Result<(u16, Vec<u8>), HostError> {
    let mut state = state.lock().await;
    let path = path.split('?').next().unwrap_or(path);
    if path.ends_with("/info") {
        return Ok((
            200,
            serde_json::to_vec(&serde_json::json!({
                "ID": state.engine_id.as_str(),
                "DockerRootDir": DOCKER_ROOT,
                "OSType": "linux",
                "Architecture": "x86_64"
            }))
            .map_err(|_| HostError::Frame)?,
        ));
    }
    let Some(reference) = path.split("/networks/").nth(1) else {
        return Ok((404, br#"{"message":"missing"}"#.to_vec()));
    };
    if method == "DELETE" {
        if reference == state.saved_id && state.present {
            state.deleted_id = Some(reference.to_owned());
            state.present = false;
            return Ok((204, Vec::new()));
        }
        return Ok((404, br#"{"message":"missing"}"#.to_vec()));
    }
    let by_name = reference == state.name && state.name_visible;
    let by_id = reference == state.saved_id;
    if by_id {
        state.saved_id_inspections += 1;
        if state.reappear_on_saved_id_inspection == Some(state.saved_id_inspections) {
            state.present = true;
        }
    }
    if !state.present || (!by_name && !by_id) {
        return Ok((404, br#"{"message":"missing"}"#.to_vec()));
    }
    let body = serde_json::json!({
        "Id": state.inspected_id.as_str(),
        "Name": state.name.as_str(),
        "Driver": "bridge",
        "Scope": "local",
        "Internal": false,
        "Attachable": false,
        "Ingress": false,
        "EnableIPv4": true,
        "EnableIPv6": false,
        "Containers": {},
        "Labels": &state.labels
    });
    Ok((
        200,
        serde_json::to_vec(&body).map_err(|_| HostError::Frame)?,
    ))
}

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use bollard::Docker;
use tokio::{net::UnixListener, sync::Mutex};

use crate::{error::HostError, journal::StorageCleanupIdentity, launch::VerifiedWorkerImages};

const ENGINE: &str = "11111111-1111-4111-8111-111111111111";
const ROOT: &str = "/var/lib/docker";
const PROBE_ID: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const DEFAULT_PROBE_CAPACITY: u64 = 19 * 1024 * 1024 * 1024;
const DEFAULT_PROBE_TOTAL: u64 = 40 * 1024 * 1024 * 1024;

pub(crate) struct StatefulDocker {
    docker: Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<()>>,
    request_count: Arc<AtomicU64>,
    effects: Arc<EffectCounts>,
    state: Arc<Mutex<ServerState>>,
}

impl StatefulDocker {
    pub(crate) fn open(
        images: &VerifiedWorkerImages,
        identity: StorageCleanupIdentity,
        probe_config: serde_json::Value,
    ) -> Result<Self, HostError> {
        Self::open_with_capacity(
            images,
            identity,
            probe_config,
            0,
            DEFAULT_PROBE_CAPACITY,
            DEFAULT_PROBE_TOTAL,
        )
    }

    pub(crate) fn open_with_capacity(
        images: &VerifiedWorkerImages,
        identity: StorageCleanupIdentity,
        probe_config: serde_json::Value,
        capacity_first: u64,
        capacity_after_first: u64,
        capacity_total: u64,
    ) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let socket_id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-cleanup-runtime-{}-{socket_id}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|_| HostError::Path)?;
        let state = Arc::new(Mutex::new(ServerState::new(
            images,
            identity,
            probe_config,
            capacity_first,
            capacity_after_first,
            capacity_total,
        )));
        let request_count = Arc::new(AtomicU64::new(0));
        let effects = Arc::new(EffectCounts::default());
        let task = tokio::spawn(server::serve(
            listener,
            Arc::clone(&state),
            Arc::clone(&request_count),
            Arc::clone(&effects),
        ));
        let socket = path.to_str().ok_or(HostError::Path)?;
        let docker = Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|_| HostError::Docker)?;
        Ok(Self {
            docker,
            path,
            task: Some(task),
            request_count,
            effects,
            state,
        })
    }

    pub(crate) fn docker(&self) -> Docker {
        self.docker.clone()
    }

    pub(crate) async fn expose_owned_containers(
        &self,
        connection_id: &str,
    ) -> Result<(), HostError> {
        self.expose_owned_containers_with_mismatch(connection_id, OwnedContainerMismatch::None)
            .await
    }

    pub(crate) async fn expose_owned_containers_with_mismatch(
        &self,
        connection_id: &str,
        mismatch: OwnedContainerMismatch,
    ) -> Result<(), HostError> {
        let mut state = self.state.lock().await;
        state.owned_connection_id = Some(connection_id.to_owned());
        state.owned_mismatch = mismatch;
        state.owned_containers_present = true;
        Ok(())
    }

    pub(crate) async fn expose_empty_runner_logs(&self) {
        self.state.lock().await.empty_runner_logs = true;
    }

    pub(crate) async fn fail_runner_log_execs(&self, count: u8) {
        self.state.lock().await.runner_log_exec_failures_remaining = count;
    }

    pub(crate) async fn runner_log_exec_count(&self) -> u8 {
        self.state.lock().await.exec_counter
    }

    pub(crate) async fn fail_owned_volume_removal(
        &self,
        connection_id: &str,
    ) -> Result<(), HostError> {
        let mut state = self.state.lock().await;
        state.owned_connection_id = Some(connection_id.to_owned());
        state.owned_volumes = state
            .identity
            .volumes()
            .iter()
            .map(|(_, name)| name.clone())
            .collect();
        state.volume_delete_failure = state
            .identity
            .volumes()
            .iter()
            .find(|(role, _)| *role == crate::journal::StorageRole::RunnerHome)
            .map(|(_, name)| name.clone());
        Ok(())
    }

    pub(crate) fn request_count(&self) -> u64 {
        self.request_count.load(Ordering::Relaxed)
    }

    pub(crate) fn effects(&self) -> EffectSnapshot {
        EffectSnapshot {
            observer_runs: self.effects.observer_runs.load(Ordering::Relaxed),
            capacity_runs: self.effects.capacity_runs.load(Ordering::Relaxed),
            container_deletes: self.effects.container_deletes.load(Ordering::Relaxed),
            volume_deletes: self.effects.volume_deletes.load(Ordering::Relaxed),
            network_deletes: self.effects.network_deletes.load(Ordering::Relaxed),
        }
    }
}

#[derive(Default)]
struct EffectCounts {
    observer_runs: AtomicU64,
    capacity_runs: AtomicU64,
    container_deletes: AtomicU64,
    volume_deletes: AtomicU64,
    network_deletes: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EffectSnapshot {
    pub(crate) observer_runs: u64,
    pub(crate) capacity_runs: u64,
    pub(crate) container_deletes: u64,
    pub(crate) volume_deletes: u64,
    pub(crate) network_deletes: u64,
}

impl Drop for StatefulDocker {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let _removed = std::fs::remove_file(&self.path).err();
    }
}

struct ServerState {
    images: Vec<ImageInfo>,
    identity: StorageCleanupIdentity,
    probe: Option<ProbeState>,
    capacity_runs: u64,
    capacity_first: u64,
    capacity_after_first: u64,
    capacity_total: u64,
    owned_connection_id: Option<String>,
    owned_mismatch: OwnedContainerMismatch,
    owned_containers_present: bool,
    removed_owned_containers: HashSet<String>,
    stopped_owned_containers: HashSet<String>,
    owned_volumes: HashSet<String>,
    volume_delete_failure: Option<String>,
    exec_counter: u8,
    exec_state: Option<ExecState>,
    empty_runner_logs: bool,
    runner_log_exec_failures_remaining: u8,
}

struct ExecState {
    id: String,
    started: bool,
    exit_code: Option<i64>,
}

#[derive(Clone, Copy, Default)]
pub(crate) enum OwnedContainerMismatch {
    #[default]
    None,
    Id,
    Name,
    Reservation,
    Operation,
    Scope,
}

impl ServerState {
    fn new(
        images: &VerifiedWorkerImages,
        identity: StorageCleanupIdentity,
        probe_config: serde_json::Value,
        capacity_first: u64,
        capacity_after_first: u64,
        capacity_total: u64,
    ) -> Self {
        Self {
            images: [
                ("velnor-runner", images.runner()),
                ("velnor-dind", images.dind()),
            ]
            .into_iter()
            .map(|(name, image)| ImageInfo::worker(name, images.source_sha(), image))
            .chain(std::iter::once(ImageInfo::probe(probe_config)))
            .collect(),
            identity,
            probe: None,
            capacity_runs: 0,
            capacity_first,
            capacity_after_first,
            capacity_total,
            owned_connection_id: None,
            owned_mismatch: OwnedContainerMismatch::None,
            owned_containers_present: false,
            removed_owned_containers: HashSet::new(),
            stopped_owned_containers: HashSet::new(),
            owned_volumes: HashSet::new(),
            volume_delete_failure: None,
            exec_counter: 0,
            exec_state: None,
            empty_runner_logs: false,
            runner_log_exec_failures_remaining: 0,
        }
    }
}

struct ImageInfo {
    id: String,
    tag: String,
    config: serde_json::Value,
    descriptor: Option<serde_json::Value>,
}

impl ImageInfo {
    fn worker(name: &str, source: &str, image: &crate::launch::VerifiedImageIdentity) -> Self {
        let tag = format!("{name}:v{source}-{}", image.archive_sha256());
        let descriptor = if image.runtime_id() == image.config_digest() {
            None
        } else {
            let (digest, media_type, size) = match image.image_index_digest() {
                Some(index) => (
                    index,
                    "application/vnd.oci.image.index.v1+json",
                    image.image_index_size().unwrap_or_default(),
                ),
                None => (
                    image.manifest_digest(),
                    "application/vnd.oci.image.manifest.v1+json",
                    image.manifest_size(),
                ),
            };
            let tag_name = format!("v{source}-{}", image.archive_sha256());
            Some(serde_json::json!({
                "digest": digest,
                "mediaType": media_type,
                "size": size,
                "annotations": {
                    "io.containerd.image.name": format!("docker.io/library/{tag}"),
                    "org.opencontainers.image.ref.name": tag_name
                }
            }))
        };
        let inspected_id = image
            .image_index_digest()
            .or_else(|| (image.runtime_id() == image.config_digest()).then_some(image.runtime_id()))
            .unwrap_or(image.manifest_digest());
        Self {
            id: inspected_id.to_owned(),
            tag,
            config: serde_json::json!({
                "Labels": {"org.opencontainers.image.revision": source}
            }),
            descriptor,
        }
    }

    fn probe(config: serde_json::Value) -> Self {
        Self {
            id: format!("sha256:{}", "a".repeat(64)),
            tag: String::new(),
            config,
            descriptor: None,
        }
    }
}

struct ProbeState {
    id: String,
    name: String,
    config: serde_json::Value,
    host_config: serde_json::Value,
    phase: ProbePhase,
    request_body: Vec<u8>,
    capacity_available: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProbePhase {
    Created,
    Running,
    Exited,
    Removed,
}

#[path = "docker/server.rs"]
mod server;

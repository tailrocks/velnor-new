use super::*;

pub(super) struct ApiCapture {
    pub(super) requests: Vec<String>,
    pub(super) stdin: Vec<u8>,
    pub(super) create_name: Option<String>,
}

pub(super) struct InitializationDockerApi {
    pub(super) docker: Docker,
    path: PathBuf,
    pub(super) capture: Arc<Mutex<ApiCapture>>,
    stop: Option<oneshot::Sender<()>>,
    release_create: Option<oneshot::Sender<()>>,
    create_seen: Option<oneshot::Receiver<()>>,
    create_completed: Option<oneshot::Receiver<()>>,
    release_response: Option<oneshot::Sender<()>>,
    response_seen: Option<oneshot::Receiver<()>>,
    release_delete_response: Option<oneshot::Sender<()>>,
    delete_seen: Option<oneshot::Receiver<()>>,
    release_inspect_response: Option<oneshot::Sender<()>>,
    inspect_seen: Option<oneshot::Receiver<()>>,
    task: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

impl InitializationDockerApi {
    pub(super) fn open(
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_mode(
            home_volume,
            work_volume,
            image_id,
            helpers::TEST_ENGINE_ID,
            None,
            None,
            false,
            None,
            false,
            false,
            None,
            None,
            false,
        )
    }

    pub(super) fn open_recovery(
        home_volume: &str,
        work_volume: &str,
        engine_id: &str,
        inspect_status: Option<u16>,
    ) -> Result<Self, HostError> {
        Self::open_with_mode(
            home_volume,
            work_volume,
            "",
            engine_id,
            inspect_status,
            None,
            false,
            None,
            false,
            false,
            None,
            None,
            false,
        )
    }

    pub(super) fn open_lost_create(
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
        foreign_helper: bool,
        remove_status: Option<u16>,
    ) -> Result<Self, HostError> {
        Self::open_with_mode(
            home_volume,
            work_volume,
            image_id,
            helpers::TEST_ENGINE_ID,
            None,
            Some(500),
            foreign_helper,
            remove_status,
            false,
            false,
            None,
            None,
            false,
        )
    }

    pub(super) fn open_lost_create_held_inspect(
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_inspect_mode(
            home_volume,
            work_volume,
            image_id,
            helpers::TEST_ENGINE_ID,
            None,
            Some(500),
            false,
            None,
            false,
            false,
            None,
            None,
            false,
            true,
        )
    }

    pub(super) fn open_held_create(
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_mode(
            home_volume,
            work_volume,
            image_id,
            helpers::TEST_ENGINE_ID,
            None,
            None,
            false,
            None,
            true,
            true,
            None,
            None,
            false,
        )
    }

    pub(super) fn open_mismatched_helper_id(
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
        observed_helper_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_mode(
            home_volume,
            work_volume,
            image_id,
            helpers::TEST_ENGINE_ID,
            None,
            None,
            false,
            None,
            false,
            false,
            Some(observed_helper_id.to_owned()),
            None,
            false,
        )
    }

    pub(super) fn open_lost_start(
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_mode(
            home_volume,
            work_volume,
            image_id,
            helpers::TEST_ENGINE_ID,
            None,
            None,
            false,
            None,
            false,
            false,
            None,
            Some(500),
            false,
        )
    }

    pub(super) fn open_lost_start_held_delete(
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_mode(
            home_volume,
            work_volume,
            image_id,
            helpers::TEST_ENGINE_ID,
            None,
            None,
            false,
            None,
            false,
            false,
            None,
            Some(500),
            true,
        )
    }

    fn open_with_mode(
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
        engine_id: &str,
        inspect_status: Option<u16>,
        create_status: Option<u16>,
        foreign_helper: bool,
        remove_status: Option<u16>,
        hold_create: bool,
        hold_response: bool,
        reported_helper_id: Option<String>,
        start_status: Option<u16>,
        hold_delete_response: bool,
    ) -> Result<Self, HostError> {
        Self::open_with_inspect_mode(
            home_volume,
            work_volume,
            image_id,
            engine_id,
            inspect_status,
            create_status,
            foreign_helper,
            remove_status,
            hold_create,
            hold_response,
            reported_helper_id,
            start_status,
            hold_delete_response,
            false,
        )
    }

    fn open_with_inspect_mode(
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
        engine_id: &str,
        inspect_status: Option<u16>,
        create_status: Option<u16>,
        foreign_helper: bool,
        remove_status: Option<u16>,
        hold_create: bool,
        hold_response: bool,
        reported_helper_id: Option<String>,
        start_status: Option<u16>,
        hold_delete_response: bool,
        hold_inspect_response: bool,
    ) -> Result<Self, HostError> {
        let path = std::env::temp_dir().join(format!("velnor-init-{}.sock", uuid::Uuid::new_v4()));
        let listener = UnixListener::bind(&path).map_err(|_| HostError::Path)?;
        let capture = Arc::new(Mutex::new(ApiCapture {
            requests: Vec::new(),
            stdin: Vec::new(),
            create_name: None,
        }));
        let server_capture = Arc::clone(&capture);
        let home_volume = home_volume.to_owned();
        let work_volume = work_volume.to_owned();
        let metadata_volume = format!(
            "{}-metadata",
            home_volume
                .strip_suffix("-home")
                .ok_or(HostError::Identity)?
        );
        let image_id = image_id.to_owned();
        let engine_id = engine_id.to_owned();
        let (
            create_gate,
            server_create_seen,
            server_create_completed,
            release_create,
            create_seen,
            create_completed,
        ) = if hold_create {
            let (release, gate) = oneshot::channel();
            let (seen_tx, seen_rx) = oneshot::channel();
            let (done_tx, done_rx) = oneshot::channel();
            (
                Some(gate),
                Some(seen_tx),
                Some(done_tx),
                Some(release),
                Some(seen_rx),
                Some(done_rx),
            )
        } else {
            (None, None, None, None, None, None)
        };
        let (response_gate, server_response_seen, release_response, response_seen) =
            if hold_response {
                let (release, gate) = oneshot::channel();
                let (seen_tx, seen_rx) = oneshot::channel();
                (Some(gate), Some(seen_tx), Some(release), Some(seen_rx))
            } else {
                (None, None, None, None)
            };
        let (delete_response_gate, server_delete_seen, release_delete_response, delete_seen) =
            if hold_delete_response {
                let (release, gate) = oneshot::channel();
                let (seen_tx, seen_rx) = oneshot::channel();
                (Some(gate), Some(seen_tx), Some(release), Some(seen_rx))
            } else {
                (None, None, None, None)
            };
        let (inspect_response_gate, server_inspect_seen, release_inspect_response, inspect_seen) =
            if hold_inspect_response {
                let (release, gate) = oneshot::channel();
                let (seen_tx, seen_rx) = oneshot::channel();
                (Some(gate), Some(seen_tx), Some(release), Some(seen_rx))
            } else {
                (None, None, None, None)
            };
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            serve_initialization_api(
                listener,
                ApiState {
                    home_volume,
                    work_volume,
                    metadata_volume,
                    image_id,
                    engine_id,
                    create_gate,
                    create_seen: server_create_seen,
                    create_completed: server_create_completed,
                    response_gate,
                    response_seen: server_response_seen,
                    delete_response_gate,
                    delete_response_seen: server_delete_seen,
                    inspect_response_gate,
                    inspect_response_seen: server_inspect_seen,
                    inspect_status,
                    create_status,
                    foreign_helper,
                    remove_status,
                    reported_helper_id,
                    start_status,
                    create_config: None,
                    helper_name: None,
                    started: false,
                    stopped: false,
                    removed: false,
                },
                server_capture,
                stopped,
            )
            .await
        });
        let socket = path.to_str().ok_or(HostError::Path)?;
        let docker = Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|_| HostError::Docker)?;
        Ok(Self {
            docker,
            path,
            capture,
            stop: Some(stop),
            release_create,
            create_seen,
            create_completed,
            release_response,
            response_seen,
            release_delete_response,
            delete_seen,
            release_inspect_response,
            inspect_seen,
            task: Some(task),
        })
    }

    pub(super) async fn wait_for_create(&mut self) -> Result<(), HostError> {
        self.create_seen
            .take()
            .ok_or(HostError::Docker)?
            .await
            .map_err(|_| HostError::Docker)
    }

    pub(super) fn release_create(&mut self) {
        if let Some(release) = self.release_create.take() {
            let _sent = release.send(());
        }
    }

    pub(super) async fn wait_for_create_completion(&mut self) -> Result<(), HostError> {
        self.create_completed
            .take()
            .ok_or(HostError::Docker)?
            .await
            .map_err(|_| HostError::Docker)
    }

    pub(super) async fn wait_for_create_response(&mut self) -> Result<(), HostError> {
        self.response_seen
            .take()
            .ok_or(HostError::Docker)?
            .await
            .map_err(|_| HostError::Docker)
    }

    pub(super) fn release_create_response(&mut self) {
        if let Some(release) = self.release_response.take() {
            let _sent = release.send(());
        }
    }

    pub(super) async fn wait_for_delete(&mut self) -> Result<(), HostError> {
        self.delete_seen
            .take()
            .ok_or(HostError::Docker)?
            .await
            .map_err(|_| HostError::Docker)
    }

    pub(super) fn release_delete_response(&mut self) {
        if let Some(release) = self.release_delete_response.take() {
            let _sent = release.send(());
        }
    }

    pub(super) async fn wait_for_inspect(&mut self) -> Result<(), HostError> {
        self.inspect_seen
            .take()
            .ok_or(HostError::Docker)?
            .await
            .map_err(|_| HostError::Docker)
    }

    pub(super) fn release_inspect_response(&mut self) {
        if let Some(release) = self.release_inspect_response.take() {
            let _sent = release.send(());
        }
    }

    pub(super) async fn finish(mut self) -> Result<ApiCapture, HostError> {
        self.release_create();
        self.release_create_response();
        self.release_delete_response();
        self.release_inspect_response();
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
        }
        let task = self.task.take().ok_or(HostError::Docker)?;
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .map_err(|_| HostError::DockerTimeout)?
            .map_err(|_| HostError::Docker)?
            .map_err(|_| HostError::Docker)?;
        std::fs::remove_file(&self.path).map_err(|_| HostError::Path)?;
        let capture = self.capture.lock().await;
        Ok(ApiCapture {
            requests: capture.requests.clone(),
            stdin: capture.stdin.clone(),
            create_name: capture.create_name.clone(),
        })
    }
}

pub(super) async fn metadata_object(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
    nonce: char,
) -> Result<crate::launch::bootstrap::VerifiedCreatedStorageObject, HostError> {
    let plan = scope
        .storage_provisioning_plan(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let names = crate::worker::WorkerIdentityNames::from_nonce(&nonce.to_string().repeat(32))?;
    let name = names.volume_name(crate::worker::WorkerVolumeRole::RunnerMetadata);
    let proof = crate::launch::bootstrap::test_created_storage_object(
        reservation_id,
        plan.operation_id(),
        plan.engine_id().to_owned(),
        plan.docker_root_digest().to_owned(),
        crate::journal::StorageRole::EngineMetadata,
        name.clone(),
        name,
    )?;
    Ok(proof)
}

impl Drop for InitializationDockerApi {
    fn drop(&mut self) {
        self.release_create();
        self.release_create_response();
        self.release_delete_response();
        self.release_inspect_response();
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let _removed = std::fs::remove_file(&self.path);
    }
}

mod server;
use server::{ApiState, serve_initialization_api};

mod http;
use http::{CapturedRequest, normalized_route, read_http_request, write_http_response};

use super::*;

#[derive(Clone)]
pub(super) struct VolumeIdentity {
    pub(super) connection_id: String,
    pub(super) reservation_id: i64,
    pub(super) operation_id: i64,
}

pub(super) async fn volume_identity(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
) -> Result<VolumeIdentity, HostError> {
    let plan = scope
        .storage_provisioning_plan(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    Ok(VolumeIdentity {
        connection_id: scope.connection_id().as_str().to_owned(),
        reservation_id,
        operation_id: plan.operation_id(),
    })
}

pub(super) struct ApiCapture {
    pub(super) requests: Vec<String>,
    pub(super) stdin: Vec<u8>,
    pub(super) metadata_inodes: Option<[u64; 3]>,
    pub(super) create_name: Option<String>,
}

/// Use the initializer test responder in a combined Docker API fixture.
pub(crate) struct InitializationTestResponder {
    state: server::ApiState,
    capture: Arc<Mutex<ApiCapture>>,
}

pub(crate) struct InitializationAttachHandle {
    operation: String,
    capture: Arc<Mutex<ApiCapture>>,
}

impl InitializationTestResponder {
    pub(crate) async fn new(
        scope: &crate::journal::ConnectionJournal,
        reservation_id: i64,
        home_volume: &str,
        work_volume: &str,
        metadata_volume: &str,
        image_id: &str,
        engine_id: &str,
    ) -> Result<Self, String> {
        let volume_identity = volume_identity(scope, reservation_id)
            .await
            .map_err(|error| error.to_string())?;
        Ok(Self {
            state: server::ApiState {
                volume_identity,
                home_volume: home_volume.to_owned(),
                work_volume: work_volume.to_owned(),
                metadata_volume: metadata_volume.to_owned(),
                image_id: image_id.to_owned(),
                engine_id: engine_id.to_owned(),
                create_gate: None,
                create_seen: None,
                create_completed: None,
                response_gate: None,
                response_seen: None,
                delete_response_gate: None,
                delete_response_seen: None,
                inspect_response_gate: None,
                inspect_response_seen: None,
                inspect_status: None,
                create_status: None,
                foreign_helper: false,
                remove_status: None,
                reported_helper_id: None,
                start_status: None,
                create_config: None,
                helper_name: None,
                lifecycle: server::ContainerLifecycle::Created,
            },
            capture: Arc::new(Mutex::new(ApiCapture {
                requests: Vec::new(),
                stdin: Vec::new(),
                metadata_inodes: None,
                create_name: None,
            })),
        })
    }

    pub(crate) async fn handle(
        &mut self,
        method: &str,
        target: &str,
        body: &[u8],
    ) -> Result<(u16, Vec<u8>), String> {
        let route = http::normalized_route(target);
        self.capture
            .lock()
            .await
            .requests
            .push(format!("{method} {route}"));
        let request = http::CapturedRequest {
            method: method.to_owned(),
            target: target.to_owned(),
            body: body.to_vec(),
        };
        let (status, response) = server::route_request(&mut self.state, &request, &route)?;
        Ok((status, response.into_bytes()))
    }

    pub(crate) async fn prepare_attach(&self) -> Result<InitializationAttachHandle, String> {
        let operation = self
            .state
            .create_config
            .as_ref()
            .and_then(|config| config["Labels"]["velnor.operation"].as_str())
            .ok_or_else(|| "Container operation label is absent".to_owned())?
            .to_owned();
        Ok(InitializationAttachHandle {
            operation,
            capture: Arc::clone(&self.capture),
        })
    }

    pub(crate) async fn metadata_inodes(&self) -> Result<[u64; 3], String> {
        self.capture
            .lock()
            .await
            .metadata_inodes
            .ok_or_else(|| "The initializer has not recorded the JobFacts identity.".to_owned())
    }
}

impl InitializationAttachHandle {
    pub(crate) async fn attach(self, stream: UnixStream) -> Result<(), String> {
        use tokio::io::AsyncWriteExt;

        let mut stream = stream;
        stream
            .write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")
            .await
            .map_err(|error| {
                format!("The test server could not write the attach upgrade. Error: {error}.")
            })?;
        server::serve_attach_stream(stream, self.operation, self.capture)
            .await
            .map_err(|error| {
                format!("The test server could not process the attach stream. Error: {error}.")
            })
    }
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

#[path = "test_support/api.rs"]
mod api;
#[path = "test_support/http.rs"]
mod http;
#[path = "test_support/lifecycle.rs"]
mod lifecycle;
#[path = "test_support/server.rs"]
mod server;

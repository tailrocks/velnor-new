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

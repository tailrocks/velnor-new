use super::server::{ApiState, serve_initialization_api};
use super::*;

struct ApiOptions {
    image_id: String,
    engine_id: String,
    inspect_status: Option<u16>,
    create_status: Option<u16>,
    foreign_helper: bool,
    remove_status: Option<u16>,
    hold: ApiHold,
    reported_helper_id: Option<String>,
    start_status: Option<u16>,
}

#[derive(Clone, Copy)]
enum ApiHold {
    None,
    CreateAndResponse,
    DeleteResponse,
    InspectResponse,
}

struct ServerGates {
    create_gate: Option<oneshot::Receiver<()>>,
    create_seen: Option<oneshot::Sender<()>>,
    create_completed: Option<oneshot::Sender<()>>,
    response_gate: Option<oneshot::Receiver<()>>,
    response_seen: Option<oneshot::Sender<()>>,
    delete_response_gate: Option<oneshot::Receiver<()>>,
    delete_response_seen: Option<oneshot::Sender<()>>,
    inspect_response_gate: Option<oneshot::Receiver<()>>,
    inspect_response_seen: Option<oneshot::Sender<()>>,
}

struct ClientGates {
    release_create: Option<oneshot::Sender<()>>,
    create_seen: Option<oneshot::Receiver<()>>,
    create_completed: Option<oneshot::Receiver<()>>,
    release_response: Option<oneshot::Sender<()>>,
    response_seen: Option<oneshot::Receiver<()>>,
    release_delete_response: Option<oneshot::Sender<()>>,
    delete_seen: Option<oneshot::Receiver<()>>,
    release_inspect_response: Option<oneshot::Sender<()>>,
    inspect_seen: Option<oneshot::Receiver<()>>,
}

struct ResponseGateChannels {
    gate: Option<oneshot::Receiver<()>>,
    seen_server: Option<oneshot::Sender<()>>,
    release_client: Option<oneshot::Sender<()>>,
    seen_client: Option<oneshot::Receiver<()>>,
}

struct ApiGates {
    server: ServerGates,
    client: ClientGates,
}

impl ApiGates {
    fn new(options: &ApiOptions) -> Self {
        let (
            create_gate,
            create_seen_server,
            create_completed_server,
            release_create,
            create_seen_client,
            create_completed_client,
        ) = if matches!(options.hold, ApiHold::CreateAndResponse) {
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
        let response = response_gate_channels(matches!(options.hold, ApiHold::CreateAndResponse));
        let delete_response =
            response_gate_channels(matches!(options.hold, ApiHold::DeleteResponse));
        let inspect_response =
            response_gate_channels(matches!(options.hold, ApiHold::InspectResponse));
        Self {
            server: ServerGates {
                create_gate,
                create_seen: create_seen_server,
                create_completed: create_completed_server,
                response_gate: response.gate,
                response_seen: response.seen_server,
                delete_response_gate: delete_response.gate,
                delete_response_seen: delete_response.seen_server,
                inspect_response_gate: inspect_response.gate,
                inspect_response_seen: inspect_response.seen_server,
            },
            client: ClientGates {
                release_create,
                create_seen: create_seen_client,
                create_completed: create_completed_client,
                release_response: response.release_client,
                response_seen: response.seen_client,
                release_delete_response: delete_response.release_client,
                delete_seen: delete_response.seen_client,
                release_inspect_response: inspect_response.release_client,
                inspect_seen: inspect_response.seen_client,
            },
        }
    }
}

fn response_gate_channels(enabled: bool) -> ResponseGateChannels {
    if !enabled {
        return ResponseGateChannels {
            gate: None,
            seen_server: None,
            release_client: None,
            seen_client: None,
        };
    }
    let (release, gate) = oneshot::channel();
    let (seen_tx, seen_rx) = oneshot::channel();
    ResponseGateChannels {
        gate: Some(gate),
        seen_server: Some(seen_tx),
        release_client: Some(release),
        seen_client: Some(seen_rx),
    }
}

impl ApiOptions {
    fn new(image_id: &str, engine_id: &str) -> Self {
        Self {
            image_id: image_id.to_owned(),
            engine_id: engine_id.to_owned(),
            inspect_status: None,
            create_status: None,
            foreign_helper: false,
            remove_status: None,
            hold: ApiHold::None,
            reported_helper_id: None,
            start_status: None,
        }
    }
}

impl InitializationDockerApi {
    pub(crate) fn open(
        volume_identity: super::VolumeIdentity,
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_options(
            volume_identity,
            home_volume,
            work_volume,
            ApiOptions::new(image_id, helpers::TEST_ENGINE_ID),
        )
    }

    pub(crate) fn open_recovery(
        volume_identity: super::VolumeIdentity,
        home_volume: &str,
        work_volume: &str,
        engine_id: &str,
        inspect_status: Option<u16>,
    ) -> Result<Self, HostError> {
        Self::open_with_options(
            volume_identity,
            home_volume,
            work_volume,
            ApiOptions {
                inspect_status,
                ..ApiOptions::new("", engine_id)
            },
        )
    }

    pub(crate) fn open_lost_create(
        volume_identity: super::VolumeIdentity,
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
        foreign_helper: bool,
        remove_status: Option<u16>,
    ) -> Result<Self, HostError> {
        Self::open_with_options(
            volume_identity,
            home_volume,
            work_volume,
            ApiOptions {
                create_status: Some(500),
                foreign_helper,
                remove_status,
                ..ApiOptions::new(image_id, helpers::TEST_ENGINE_ID)
            },
        )
    }

    pub(crate) fn open_lost_create_held_inspect(
        volume_identity: super::VolumeIdentity,
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_options(
            volume_identity,
            home_volume,
            work_volume,
            ApiOptions {
                create_status: Some(500),
                hold: ApiHold::InspectResponse,
                ..ApiOptions::new(image_id, helpers::TEST_ENGINE_ID)
            },
        )
    }

    pub(crate) fn open_held_create(
        volume_identity: super::VolumeIdentity,
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_options(
            volume_identity,
            home_volume,
            work_volume,
            ApiOptions {
                hold: ApiHold::CreateAndResponse,
                ..ApiOptions::new(image_id, helpers::TEST_ENGINE_ID)
            },
        )
    }

    pub(crate) fn open_mismatched_helper_id(
        volume_identity: super::VolumeIdentity,
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
        observed_helper_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_options(
            volume_identity,
            home_volume,
            work_volume,
            ApiOptions {
                reported_helper_id: Some(observed_helper_id.to_owned()),
                ..ApiOptions::new(image_id, helpers::TEST_ENGINE_ID)
            },
        )
    }

    pub(crate) fn open_lost_start(
        volume_identity: super::VolumeIdentity,
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_options(
            volume_identity,
            home_volume,
            work_volume,
            ApiOptions {
                start_status: Some(500),
                ..ApiOptions::new(image_id, helpers::TEST_ENGINE_ID)
            },
        )
    }

    pub(crate) fn open_lost_start_held_delete(
        volume_identity: super::VolumeIdentity,
        home_volume: &str,
        work_volume: &str,
        image_id: &str,
    ) -> Result<Self, HostError> {
        Self::open_with_options(
            volume_identity,
            home_volume,
            work_volume,
            ApiOptions {
                start_status: Some(500),
                hold: ApiHold::DeleteResponse,
                ..ApiOptions::new(image_id, helpers::TEST_ENGINE_ID)
            },
        )
    }

    fn open_with_options(
        volume_identity: super::VolumeIdentity,
        home_volume: &str,
        work_volume: &str,
        options: ApiOptions,
    ) -> Result<Self, HostError> {
        let gates = ApiGates::new(&options);
        let ApiGates { server, client } = gates;
        let ApiOptions {
            image_id,
            engine_id,
            inspect_status,
            create_status,
            foreign_helper,
            remove_status,
            reported_helper_id,
            start_status,
            ..
        } = options;
        let path = std::env::temp_dir().join(format!("velnor-init-{}.sock", uuid::Uuid::new_v4()));
        let listener = UnixListener::bind(&path).map_err(|_| HostError::Path)?;
        let capture = Arc::new(Mutex::new(ApiCapture {
            requests: Vec::new(),
            stdin: Vec::new(),
            metadata_inodes: None,
            create_name: None,
        }));
        let server_capture = Arc::clone(&capture);
        let home_volume = home_volume.to_owned();
        let work_volume = work_volume.to_owned();
        let metadata_volume = metadata_volume_name(&home_volume)?;
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            serve_initialization_api(
                listener,
                ApiState {
                    volume_identity,
                    home_volume,
                    work_volume,
                    metadata_volume,
                    image_id,
                    engine_id,
                    create_gate: server.create_gate,
                    create_seen: server.create_seen,
                    create_completed: server.create_completed,
                    response_gate: server.response_gate,
                    response_seen: server.response_seen,
                    delete_response_gate: server.delete_response_gate,
                    delete_response_seen: server.delete_response_seen,
                    inspect_response_gate: server.inspect_response_gate,
                    inspect_response_seen: server.inspect_response_seen,
                    inspect_status,
                    create_status,
                    foreign_helper,
                    remove_status,
                    reported_helper_id,
                    start_status,
                    create_config: None,
                    helper_name: None,
                    lifecycle: super::server::ContainerLifecycle::Created,
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
            release_create: client.release_create,
            create_seen: client.create_seen,
            create_completed: client.create_completed,
            release_response: client.release_response,
            response_seen: client.response_seen,
            release_delete_response: client.release_delete_response,
            delete_seen: client.delete_seen,
            release_inspect_response: client.release_inspect_response,
            inspect_seen: client.inspect_seen,
            task: Some(task),
        })
    }
}

fn metadata_volume_name(home_volume: &str) -> Result<String, HostError> {
    let base = home_volume
        .strip_suffix("-home")
        .ok_or(HostError::Identity)?;
    Ok(format!("{base}-metadata"))
}

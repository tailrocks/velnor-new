use std::collections::HashMap;

use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{Mutex as AsyncMutex, oneshot},
};

use super::*;

#[path = "daemon/http.rs"]
mod http;

use self::http::read_request;

#[path = "daemon/publication.rs"]
mod publication;

#[path = "daemon/exec_protocol.rs"]
mod exec_protocol;

#[path = "daemon/state_changes.rs"]
mod state_changes;

#[path = "daemon/runner_failure.rs"]
mod runner_failure;

#[path = "exec_fixture.rs"]
mod exec_fixture;

pub(super) async fn serve(
    listener: UnixListener,
    images: crate::launch::VerifiedWorkerImages,
    initializer: Option<(
        String,
        Arc<AsyncMutex<crate::worker::storage_initialization::InitializationTestResponder>>,
    )>,
    runner_create_status: Option<u16>,
    requests: Arc<std::sync::Mutex<Vec<CapturedRequest>>>,
    jit_input: Arc<std::sync::Mutex<Vec<u8>>>,
    mut stop: oneshot::Receiver<bool>,
) -> Result<(), String> {
    let mut state = ServerState::new(
        images,
        initializer,
        runner_create_status,
        requests,
        jit_input,
    );
    loop {
        let accepted = tokio::select! {
            drain_open_attachments = &mut stop => {
                state.finish(drain_open_attachments.unwrap_or(false)).await?;
                return Ok(());
            },
            accepted = tokio::time::timeout(TIMEOUT, listener.accept()) => accepted,
        };
        let (stream, _) = accepted
            .map_err(|_| "Docker API request timed out".to_owned())?
            .map_err(|error| error.to_string())?;
        state.handle_stream(stream).await?;
    }
}

struct ServerState {
    images: crate::launch::VerifiedWorkerImages,
    initializer: Option<(
        String,
        Arc<AsyncMutex<crate::worker::storage_initialization::InitializationTestResponder>>,
    )>,
    runner_create_status: Option<u16>,
    initializer_container_id: Option<String>,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    jit_input: Arc<Mutex<Vec<u8>>>,
    volumes: HashMap<String, Value>,
    networks: HashMap<String, Value>,
    containers: HashMap<String, Value>,
    publication_logs: Arc<AsyncMutex<HashMap<String, Vec<u8>>>>,
    attach_tasks: Vec<tokio::task::JoinHandle<Result<(), String>>>,
    execs: Arc<AsyncMutex<HashMap<String, ExecRecord>>>,
}

#[derive(Clone)]
struct ExecRecord {
    container_id: String,
    pid: u32,
    running: bool,
}

impl ServerState {
    fn new(
        images: crate::launch::VerifiedWorkerImages,
        initializer: Option<(
            String,
            Arc<AsyncMutex<crate::worker::storage_initialization::InitializationTestResponder>>,
        )>,
        runner_create_status: Option<u16>,
        requests: Arc<std::sync::Mutex<Vec<CapturedRequest>>>,
        jit_input: Arc<std::sync::Mutex<Vec<u8>>>,
    ) -> Self {
        Self {
            images,
            initializer,
            runner_create_status,
            initializer_container_id: None,
            requests,
            jit_input,
            volumes: HashMap::new(),
            networks: HashMap::new(),
            containers: HashMap::new(),
            publication_logs: Arc::new(AsyncMutex::new(HashMap::new())),
            attach_tasks: Vec::new(),
            execs: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    async fn finish(self, drain_open_attachments: bool) -> Result<(), String> {
        for task in self.attach_tasks {
            if !drain_open_attachments {
                task.abort();
            }
            match task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(error),
                Err(error) if !drain_open_attachments && error.is_cancelled() => {}
                Err(error) => {
                    return Err(format!("The Docker attach task failed. Error: {error}."));
                }
            }
        }
        Ok(())
    }

    async fn handle_stream(&mut self, mut stream: UnixStream) -> Result<(), String> {
        let request = read_request(&mut stream).await?;
        let request_index = {
            let mut requests = self
                .requests
                .lock()
                .map_err(|_| "The Docker request list lock failed.".to_owned())?;
            requests.push(request.clone());
            requests.len() - 1
        };
        if request.method == "POST" && route(&request).starts_with("/exec/") {
            self.start_exec(stream, &request, request_index).await?;
            return Ok(());
        }
        if request.method == "POST" && route(&request).ends_with("/attach") {
            self.start_attach(stream, &request).await?;
            return Ok(());
        }
        let response = self.reply(&request).await?;
        {
            let mut requests = self
                .requests
                .lock()
                .map_err(|_| "The Docker request list lock failed.".to_owned())?;
            let recorded = requests
                .get_mut(request_index)
                .ok_or_else(|| "The Docker request record is absent.".to_owned())?;
            recorded.response_status = Some(response.0);
            recorded.response_body = response.1.clone();
        }
        write_response(&mut stream, response.0, &response.1).await
    }

    async fn start_attach(
        &mut self,
        mut stream: UnixStream,
        request: &CapturedRequest,
    ) -> Result<(), String> {
        if self.is_initializer_container(request) {
            let (_, responder) = self.initializer()?;
            let attachment = responder.lock().await.prepare_attach().await?;
            self.attach_tasks.push(tokio::spawn(async move {
                attachment
                    .attach(stream)
                    .await
                    .map_err(|error| format!("The initializer attach failed. Error: {error}."))
            }));
        } else {
            if self.is_execution_publication_attach(request) {
                self.start_execution_publication_attach(stream, request)
                    .await?;
                return Ok(());
            } else if self.is_owned_cgroup_probe_attach(request) {
                self.start_owned_cgroup_attach(stream).await?;
                return Ok(());
            } else if !super::super::request_checks::valid_runner_attach(request) {
                return Err(format!(
                    "The Docker attach target is incorrect: {} {}",
                    request.method, request.target
                ));
            }
            let jit_input = Arc::clone(&self.jit_input);
            self.attach_tasks.push(tokio::spawn(async move {
                record_jit_input(&mut stream, &jit_input)
                    .await
                    .map_err(|error| format!("The Runner attach failed. Error: {error}."))
            }));
        }
        Ok(())
    }

    async fn reply(&mut self, request: &CapturedRequest) -> Result<(u16, Vec<u8>), String> {
        let initializer_create = self.is_initializer_create(request);
        if initializer_create || self.is_initializer_container(request) {
            return self.reply_initializer(request, initializer_create).await;
        }
        if self.is_execution_publication_logs(request) {
            return self.execution_publication_logs(request).await;
        }
        self.reply_docker(request)
    }

    async fn reply_initializer(
        &mut self,
        request: &CapturedRequest,
        initializer_create: bool,
    ) -> Result<(u16, Vec<u8>), String> {
        let (_, responder) = self.initializer()?;
        let response = responder
            .lock()
            .await
            .handle(&request.method, &request.target, &request.body)
            .await?;
        if initializer_create {
            self.initializer_container_id = serde_json::from_slice::<Value>(&response.1)
                .ok()
                .and_then(|value| value["Id"].as_str().map(str::to_owned));
        }
        Ok(response)
    }

    fn reply_docker(&mut self, request: &CapturedRequest) -> Result<(u16, Vec<u8>), String> {
        if let Some(response) = runner_failure::create_rejection(self.runner_create_status, request)
        {
            return Ok(response);
        }
        if request.method == "POST"
            && route(request).starts_with("/containers/")
            && route(request).ends_with("/exec")
        {
            return self.create_exec(request);
        }
        if request.method == "GET"
            && route(request).starts_with("/exec/")
            && route(request).ends_with("/json")
        {
            return self.inspect_exec(request);
        }
        let initializer_image = self.initializer.as_ref().map(|(image, _)| image.as_str());
        let response = super::responses::answer(
            &self.images,
            initializer_image,
            request,
            &self.volumes,
            &self.networks,
            &self.containers,
        )?;
        self.record_docker_change(request, &response)?;
        Ok(response)
    }

    fn record_volume(&mut self, response: &(u16, Vec<u8>)) -> Result<(), String> {
        let value: Value =
            serde_json::from_slice(&response.1).map_err(|error| error.to_string())?;
        let name = value["Name"]
            .as_str()
            .ok_or_else(|| "Created volume has no name".to_owned())?;
        self.volumes.insert(name.to_owned(), value);
        Ok(())
    }

    fn record_network(&mut self, request: &CapturedRequest) -> Result<(), String> {
        let created: Value =
            serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
        let name = created["Name"]
            .as_str()
            .ok_or_else(|| "Created network has no name".to_owned())?;
        let id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        self.networks.insert(
            name.to_owned(),
            json!({
                "Id": id,
                "Name": name,
                "Driver": created["Driver"],
                "Scope": created["Scope"],
                "Internal": created["Internal"],
                "Attachable": created["Attachable"],
                "Ingress": created["Ingress"],
                "EnableIPv4": created["EnableIPv4"],
                "EnableIPv6": created["EnableIPv6"],
                "Labels": created["Labels"],
                "Containers": {}
            }),
        );
        Ok(())
    }

    fn record_container(
        &mut self,
        request: &CapturedRequest,
        response: &(u16, Vec<u8>),
    ) -> Result<(), String> {
        let created: Value =
            serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
        let name = request
            .target
            .split_once("name=")
            .map(|(_, value)| value.split('&').next().unwrap_or(value))
            .ok_or_else(|| "The created container name is absent.".to_owned())?;
        let id = super::responses::response_id(&response.1)?;
        let inspection =
            super::responses::container_inspect(&self.images, &created, &self.volumes, &id, name)?;
        self.containers.insert(name.to_owned(), inspection.clone());
        self.containers.insert(id, inspection);
        Ok(())
    }

    fn is_initializer_create(&self, request: &CapturedRequest) -> bool {
        self.initializer.as_ref().is_some_and(|(image, _)| {
            request.method == "POST"
                && route(request) == "/containers/create"
                && serde_json::from_slice::<Value>(&request.body)
                    .ok()
                    .is_some_and(|body| {
                        body["Image"].as_str() == Some(image)
                            && body["Labels"]["velnor.role"].as_str() == Some("storage-initializer")
                            && body["Labels"]["velnor.operation"].as_str().is_some()
                    })
        })
    }

    fn is_initializer_container(&self, request: &CapturedRequest) -> bool {
        let path = route(request);
        path.starts_with("/containers/velnor-init-")
            || self
                .initializer_container_id
                .as_ref()
                .is_some_and(|id| path.contains(id))
    }

    fn initializer(
        &self,
    ) -> Result<
        &(
            String,
            Arc<AsyncMutex<crate::worker::storage_initialization::InitializationTestResponder>>,
        ),
        String,
    > {
        self.initializer
            .as_ref()
            .ok_or_else(|| "The initializer API configuration is absent.".to_owned())
    }
}

async fn record_jit_input(
    stream: &mut UnixStream,
    jit_input: &std::sync::Mutex<Vec<u8>>,
) -> Result<(), String> {
    stream
        .write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")
        .await
        .map_err(|error| error.to_string())?;
    let mut payload = Vec::new();
    stream
        .read_to_end(&mut payload)
        .await
        .map_err(|error| error.to_string())?;
    jit_input
        .lock()
        .map_err(|_| "The JIT configuration lock failed.".to_owned())?
        .extend(payload);
    Ok(())
}

async fn write_response(stream: &mut UnixStream, status: u16, body: &[u8]) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        404 => "Not Found",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(body)
        .await
        .map_err(|error| error.to_string())
}

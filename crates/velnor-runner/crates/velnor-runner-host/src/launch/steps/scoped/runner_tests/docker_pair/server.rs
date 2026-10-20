use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bollard::Docker;
use tokio::net::UnixListener;
use tokio::sync::{Mutex as AsyncMutex, oneshot};

const TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const DIND_ID: &str = concat!(
    "dddddddddddddddd",
    "dddddddddddddddd",
    "dddddddddddddddd",
    "dddddddddddddddd"
);
pub(super) const RUNNER_ID: &str = concat!(
    "aaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaa"
);

#[path = "server/daemon.rs"]
mod daemon;
#[path = "server/publication_fixture.rs"]
mod publication_fixture;
#[path = "server/responses.rs"]
mod responses;

#[derive(Clone)]
pub(crate) struct CapturedRequest {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) body: Vec<u8>,
    pub(crate) response_status: Option<u16>,
    pub(crate) response_body: Vec<u8>,
}

pub(crate) struct CapturedApi {
    pub(crate) requests: Vec<CapturedRequest>,
    pub(crate) jit_input: Vec<u8>,
    pub(crate) server_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ActivitySnapshot {
    pub(super) requests: usize,
    pub(super) starts: usize,
    pub(super) publisher_containers: usize,
    pub(super) publisher_logs: usize,
    pub(super) runner_attaches: usize,
    pub(super) jit_input_bytes: usize,
}

pub(super) struct PairDockerApi {
    pub(super) docker: Docker,
    path: PathBuf,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    jit_input: Arc<Mutex<Vec<u8>>>,
    stop: Option<oneshot::Sender<bool>>,
    task: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

impl PairDockerApi {
    pub(super) fn open(images: crate::launch::VerifiedWorkerImages) -> Result<Self, String> {
        Self::open_with_optional_initializer(images, None, None)
    }

    pub(super) fn open_with_initializer(
        images: crate::launch::VerifiedWorkerImages,
        initializer_image_id: String,
        initializer: crate::worker::storage_initialization::InitializationTestResponder,
    ) -> Result<Self, String> {
        Self::open_with_initializer_and_runner_create_status(
            images,
            initializer_image_id,
            initializer,
            None,
        )
    }

    pub(super) fn open_with_runner_create_rejection(
        images: crate::launch::VerifiedWorkerImages,
        initializer_image_id: String,
        initializer: crate::worker::storage_initialization::InitializationTestResponder,
        status: u16,
    ) -> Result<Self, String> {
        if !matches!(status, 400 | 404 | 409) {
            return Err(
                "A rejected Runner container request must return a direct Docker 400, 404, or 409 response."
                    .to_owned(),
            );
        }
        Self::open_with_initializer_and_runner_create_status(
            images,
            initializer_image_id,
            initializer,
            Some(status),
        )
    }

    fn open_with_initializer_and_runner_create_status(
        images: crate::launch::VerifiedWorkerImages,
        initializer_image_id: String,
        initializer: crate::worker::storage_initialization::InitializationTestResponder,
        runner_create_status: Option<u16>,
    ) -> Result<Self, String> {
        let initializer = Arc::new(AsyncMutex::new(initializer));
        Self::open_with_optional_initializer(
            images,
            Some((initializer_image_id, Arc::clone(&initializer))),
            runner_create_status,
        )
    }

    fn open_with_optional_initializer(
        images: crate::launch::VerifiedWorkerImages,
        initializer: Option<(
            String,
            Arc<AsyncMutex<crate::worker::storage_initialization::InitializationTestResponder>>,
        )>,
        runner_create_status: Option<u16>,
    ) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("vp-{}.sock", uuid::Uuid::new_v4()));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let jit_input = Arc::new(Mutex::new(Vec::new()));
        let (stop, stopped) = oneshot::channel();
        let server_requests = Arc::clone(&requests);
        let server_jit = Arc::clone(&jit_input);
        let task = tokio::spawn(async move {
            daemon::serve(
                listener,
                images,
                initializer,
                runner_create_status,
                server_requests,
                server_jit,
                stopped,
            )
            .await
        });
        let socket = path
            .to_str()
            .ok_or_else(|| "Docker socket path is not UTF-8".to_owned())?;
        let docker = Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            docker,
            path,
            requests,
            jit_input,
            stop: Some(stop),
            task: Some(task),
        })
    }

    pub(super) async fn finish(self) -> Result<CapturedApi, String> {
        self.finish_with_attach_drain(true).await
    }

    pub(super) async fn finish_partial_start(self) -> Result<CapturedApi, String> {
        self.finish_with_attach_drain(false).await
    }

    async fn finish_with_attach_drain(
        mut self,
        drain_open_attachments: bool,
    ) -> Result<CapturedApi, String> {
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(drain_open_attachments);
        }
        let mut task = self
            .task
            .take()
            .ok_or_else(|| "The Docker API test server stopped.".to_owned())?;
        let server_error = match tokio::time::timeout(TIMEOUT, &mut task).await {
            Ok(Ok(Ok(()))) => None,
            Ok(Ok(Err(error))) => Some(error),
            Ok(Err(error)) => Some(format!("The Docker API task failed. Error: {error}.")),
            Err(_) => Some("The Docker API test server timed out.".to_owned()),
        };
        std::fs::remove_file(&self.path).map_err(|error| {
            format!("The Docker API socket could not be removed. Error: {error}.")
        })?;
        Ok(CapturedApi {
            requests: take(&self.requests, "The Docker request list lock failed.")?,
            jit_input: take(&self.jit_input, "The JIT configuration lock failed.")?,
            server_error,
        })
    }

    pub(super) fn request_summary(&self) -> Result<Vec<String>, String> {
        self.requests
            .lock()
            .map_err(|_| "The Docker request list lock failed.".to_owned())
            .map(|requests| {
                let mut summary: Vec<String> = requests
                    .iter()
                    .rev()
                    .take(8)
                    .map(|request| {
                        format!(
                            "{} {} -> {:?}",
                            request.method, request.target, request.response_status
                        )
                    })
                    .collect();
                summary.reverse();
                summary
            })
    }

    pub(super) fn activity_snapshot(&self) -> Result<ActivitySnapshot, String> {
        let requests = self
            .requests
            .lock()
            .map_err(|_| "The Docker request list lock failed.".to_owned())?;
        let publisher_containers = requests
            .iter()
            .filter(|request| {
                request.method == "POST"
                    && route(request) == "/containers/create"
                    && serde_json::from_slice::<serde_json::Value>(&request.body)
                        .ok()
                        .is_some_and(|body| {
                            body["Labels"]["velnor.role"] == "execution-publication"
                        })
            })
            .count();
        let snapshot = ActivitySnapshot {
            requests: requests.len(),
            starts: requests
                .iter()
                .filter(|request| {
                    request.method == "POST"
                        && route(request).starts_with("/containers/")
                        && route(request).ends_with("/start")
                })
                .count(),
            publisher_containers,
            publisher_logs: requests
                .iter()
                .filter(|request| {
                    request.method == "GET"
                        && route(request).starts_with("/containers/")
                        && route(request).ends_with("/logs")
                })
                .count(),
            runner_attaches: requests
                .iter()
                .filter(|request| super::request_checks::valid_runner_attach(request))
                .count(),
            jit_input_bytes: self
                .jit_input
                .lock()
                .map_err(|_| "The JIT configuration lock failed.".to_owned())?
                .len(),
        };
        Ok(snapshot)
    }
}

impl Drop for PairDockerApi {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(false);
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let _removed = std::fs::remove_file(&self.path);
    }
}

pub(super) fn route(request: &CapturedRequest) -> &str {
    let path = request.target.split('?').next().unwrap_or(&request.target);
    if path.starts_with("/v") && path.as_bytes().get(2).is_some_and(u8::is_ascii_digit) {
        path[1..].find('/').map_or(path, |index| &path[index + 1..])
    } else {
        path
    }
}

fn take<T: Default>(value: &Mutex<T>, message: &'static str) -> Result<T, String> {
    value
        .lock()
        .map_err(|_| message.to_owned())
        .map(|mut value| std::mem::take(&mut *value))
}

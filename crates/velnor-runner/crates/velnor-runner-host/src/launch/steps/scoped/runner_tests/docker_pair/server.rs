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
#[path = "server/responses.rs"]
mod responses;

#[derive(Clone)]
pub(super) struct CapturedRequest {
    pub(super) method: String,
    pub(super) target: String,
    pub(super) body: Vec<u8>,
    pub(super) response_status: Option<u16>,
    pub(super) response_body: Vec<u8>,
}

pub(super) struct CapturedApi {
    pub(super) requests: Vec<CapturedRequest>,
    pub(super) jit_input: Vec<u8>,
    pub(super) server_error: Option<String>,
}

pub(super) struct PairDockerApi {
    pub(super) docker: Docker,
    path: PathBuf,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    jit_input: Arc<Mutex<Vec<u8>>>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

impl PairDockerApi {
    pub(super) fn open(images: crate::launch::VerifiedWorkerImages) -> Result<Self, String> {
        Self::open_with_optional_initializer(images, None)
    }

    pub(super) fn open_with_initializer(
        images: crate::launch::VerifiedWorkerImages,
        initializer_image_id: String,
        initializer: crate::worker::storage_initialization::InitializationTestResponder,
    ) -> Result<Self, String> {
        let initializer = Arc::new(AsyncMutex::new(initializer));
        Self::open_with_optional_initializer(
            images,
            Some((initializer_image_id, Arc::clone(&initializer))),
        )
    }

    fn open_with_optional_initializer(
        images: crate::launch::VerifiedWorkerImages,
        initializer: Option<(
            String,
            Arc<AsyncMutex<crate::worker::storage_initialization::InitializationTestResponder>>,
        )>,
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

    pub(super) async fn finish(mut self) -> Result<CapturedApi, String> {
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
        }
        let mut task = self
            .task
            .take()
            .ok_or_else(|| "Docker API test server stopped".to_owned())?;
        let server_error = match tokio::time::timeout(TIMEOUT, &mut task).await {
            Ok(Ok(Ok(()))) => None,
            Ok(Ok(Err(error))) => Some(error),
            Ok(Err(error)) => Some(format!("Docker API task failed: {error}")),
            Err(_) => Some("Docker API test server timed out".to_owned()),
        };
        std::fs::remove_file(&self.path).map_err(|error| error.to_string())?;
        Ok(CapturedApi {
            requests: take(&self.requests, "Docker request list is poisoned")?,
            jit_input: take(&self.jit_input, "JIT input is poisoned")?,
            server_error,
        })
    }
}

impl Drop for PairDockerApi {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
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

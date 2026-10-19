//! Shared fixtures for completion cleanup tests.

#[path = "container_tests/diagnostics.rs"]
mod diagnostics;
#[path = "container_tests/identity.rs"]
mod identity;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use bollard::Docker;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::super::volume;
use super::cleanup;
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::journal::{CleanupClaim, CompletedLaunch, Journal};
use crate::launch::completion::cleanup::{
    Context, EffectBudget, Failure, cleanup_retry_delay_seconds, completion_now,
};
use crate::launch::completion::diagnostics::{DiagnosticsRoot, SCOPE_DIRECTORY, scope_component};
use crate::launch::completion::journal_ref::CompletionJournal;
use crate::listen::Secret;

const WORKER: &str = "wcompletion";
const RUNNER_ID: &str = "runner-id";
const DIND_ID: &str = "dind-id";
const TEST_TIMEOUT: Duration = Duration::from_secs(2);

async fn run_cleanup(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
) -> Result<(), Failure> {
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);
    cleanup(context, launch, claim, &mut budget).await
}

async fn completed_launch(journal: &Journal) -> Result<CompletedLaunch, HostError> {
    let request_id = 513;
    let subject = format!("m{request_id}r{request_id}");
    let runner_name = format!("v{request_id}");
    let (id, _) = journal
        .begin_assigned_launch(&subject, 77, request_id, &runner_name)
        .await?;
    if journal
        .record_runner_completed(77, request_id, request_id + 10_000, &runner_name)
        .await?
        != Some(id)
    {
        return Err(HostError::Journal);
    }
    let mut launches = journal.due_completed_launches(completion_now()?, 4).await?;
    let mut launch = launches.pop().ok_or(HostError::Journal)?;
    launch.intent.worker_volume = Some(WORKER.to_owned());
    launch.intent.docker_id = Some(RUNNER_ID.to_owned());
    launch.intent.dind_id = Some(DIND_ID.to_owned());
    Ok(launch)
}

fn diagnostics_root(scratch: &Scratch) -> Result<DiagnosticsRoot, HostError> {
    let state = std::fs::canonicalize(&scratch.directory).map_err(|_| HostError::Path)?;
    DiagnosticsRoot::open(&state, "root")
}

fn diagnostic_filename(intent_id: i64, generation: i64, role: &str, container_id: &str) -> String {
    let digest = Sha256::digest(container_id.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ignored = write!(hex, "{byte:02x}");
    }
    format!("i{intent_id}-g{generation}-{role}-{hex}.json")
}

fn container_json(id: &str, role: &str) -> String {
    serde_json::json!({
        "Id": id,
        "Name": format!("/{WORKER}-{role}"),
        "Image": "sha256:0123456789abcdef",
        "RestartCount": 2,
        "State": {
            "Status": "exited",
            "Running": false,
            "Paused": false,
            "Restarting": false,
            "OOMKilled": false,
            "Dead": false,
            "ExitCode": 1,
            "Error": "secret-state-error",
            "StartedAt": "2026-10-19T00:00:00Z",
            "FinishedAt": "2026-10-19T00:01:00Z",
            "Health": {"Log": [{"Output": "secret-health-output"}]}
        },
        "Config": {
            "Labels": {
                "velnor.volume": WORKER,
                "velnor.worker": WORKER,
                "velnor.role": role,
                "secret-label": "secret-label-value"
            },
            "Env": ["SECRET_ENV=secret-env-value"]
        }
    })
    .to_string()
}

fn volume_json(name: &str, worker: &str, role: &str) -> String {
    serde_json::json!({
        "Name": name,
        "Driver": "local",
        "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"),
        "Labels": {"velnor.worker": worker, "velnor.role": role},
        "Options": {},
        "Scope": "local"
    })
    .to_string()
}

fn response(status: u16, body: &str) -> Response {
    Response {
        status,
        body: body.to_owned(),
    }
}

struct Response {
    status: u16,
    body: String,
}

struct DockerStub {
    docker: Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<Vec<String>, String>>>,
}

impl DockerStub {
    fn open(responses: Vec<Response>) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-container-cleanup-{}-{id}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|_| HostError::Path)?;
        let task = tokio::spawn(serve(listener, responses, None));
        let socket = path.to_str().ok_or(HostError::Path)?;
        let docker = Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|_| HostError::Docker)?;
        Ok(Self {
            docker,
            path,
            task: Some(task),
        })
    }

    fn open_paused(
        responses: Vec<Response>,
        request_index: usize,
    ) -> Result<
        (
            Self,
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        HostError,
    > {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-container-cleanup-paused-{}-{id}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|_| HostError::Path)?;
        let (reached_tx, reached_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let gate = RequestGate {
            request_index,
            reached: reached_tx,
            release: release_rx,
        };
        let task = tokio::spawn(serve(listener, responses, Some(gate)));
        let socket = path.to_str().ok_or(HostError::Path)?;
        let docker = Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|_| HostError::Docker)?;
        Ok((
            Self {
                docker,
                path,
                task: Some(task),
            },
            reached_rx,
            release_tx,
        ))
    }

    async fn finish(mut self) -> Result<Vec<String>, HostError> {
        let mut task = self.task.take().ok_or(HostError::Journal)?;
        let Ok(result) = tokio::time::timeout(TEST_TIMEOUT, &mut task).await else {
            task.abort();
            return Err(HostError::Docker);
        };
        result
            .map_err(|_| HostError::Docker)?
            .map_err(|_| HostError::Docker)
    }
}

impl Drop for DockerStub {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let _removed = std::fs::remove_file(&self.path)
            .err()
            .map(|error| error.kind());
    }
}

struct RequestGate {
    request_index: usize,
    reached: tokio::sync::oneshot::Sender<()>,
    release: tokio::sync::oneshot::Receiver<()>,
}

async fn serve(
    listener: UnixListener,
    responses: Vec<Response>,
    mut gate: Option<RequestGate>,
) -> Result<Vec<String>, String> {
    let mut requests = Vec::with_capacity(responses.len());
    for (index, response) in responses.into_iter().enumerate() {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        requests.push(read_request_line(&mut stream).await?);
        if gate
            .as_ref()
            .is_some_and(|pending| pending.request_index == index)
        {
            let pending = gate
                .take()
                .ok_or_else(|| "request gate missing".to_owned())?;
            pending
                .reached
                .send(())
                .map_err(|()| "request gate receiver closed".to_owned())?;
            pending
                .release
                .await
                .map_err(|_| "request gate release closed".to_owned())?;
        }
        send_response(&mut stream, response).await?;
    }
    Ok(requests)
}

async fn read_request_line(stream: &mut UnixStream) -> Result<String, String> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("Docker client closed before request headers".to_owned());
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&request).map_err(|error| error.to_string())?;
            return headers
                .split("\r\n")
                .next()
                .map(str::to_owned)
                .ok_or_else(|| "Docker request omitted its request line".to_owned());
        }
    }
}

async fn send_response(stream: &mut UnixStream, response: Response) -> Result<(), String> {
    let message = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        reason(response.status),
        response.body.len(),
        response.body
    );
    stream
        .write_all(message.as_bytes())
        .await
        .map_err(|error| error.to_string())
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

struct Scratch {
    path: PathBuf,
    directory: PathBuf,
}

impl Scratch {
    fn open(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "velnor-container-cleanup-{label}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).map_err(|_| HostError::Path)?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| HostError::Path)?;
        Ok(Self {
            path: directory.join("journal.db"),
            directory,
        })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.directory)
            .err()
            .map(|error| error.kind());
    }
}

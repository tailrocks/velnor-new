//! Retry identity checks when removed containers are already absent.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::cleanup;
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::journal::{CleanupClaim, CompletedLaunch, Journal};
use crate::launch::completion::cleanup::{
    Context, EffectBudget, Failure, cleanup_retry_delay_seconds, completion_now,
};
use crate::listen::Secret;

const WORKER: &str = "wcompletion";
const RUNNER_ID: &str = "runner-id";
const DIND_ID: &str = "dind-id";
const TEST_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn retry_accepts_runner_absent_by_recorded_id_and_name_after_dind_failure()
-> Result<(), HostError> {
    let scratch = Scratch::open("retry-absent")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 2)
        .await?
        .ok_or(HostError::Journal)?;
    let stub = DockerStub::open(vec![
        response(200, &container_json(RUNNER_ID, "runner")),
        response(200, &container_json(RUNNER_ID, "runner")),
        response(204, ""),
        response(404, r#"{"message":"missing"}"#),
        response(404, r#"{"message":"missing"}"#),
        response(200, &container_json(DIND_ID, "dind")),
        response(200, &container_json(DIND_ID, "dind")),
        response(500, r#"{"message":"busy"}"#),
        response(404, r#"{"message":"missing"}"#),
        response(404, r#"{"message":"missing"}"#),
        response(200, &container_json(DIND_ID, "dind")),
        response(200, &container_json(DIND_ID, "dind")),
        response(204, ""),
        response(404, r#"{"message":"missing"}"#),
        response(404, r#"{"message":"missing"}"#),
    ])?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("test token");
    let context = Context {
        journal: &journal,
        docker: &stub.docker,
        transport: &mut transport,
        admin: &admin,
    };

    let first_result = run_cleanup(&context, &launch, claim).await;
    assert_eq!(
        first_result,
        Err(Failure::request("container delete", HostError::Docker))
    );
    let retry_delay = cleanup_retry_delay_seconds(claim.attempt);
    if !journal
        .retry_completion_cleanup(launch.intent.id, claim.generation, retry_delay)
        .await?
    {
        return Err(HostError::Journal);
    }
    let retry_at = completion_now()?
        .checked_add(retry_delay)
        .ok_or(HostError::Journal)?;
    let retry_claim = journal
        .claim_completion_cleanup_at(launch.intent.id, retry_at, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let retry_result = run_cleanup(&context, &launch, retry_claim).await;
    assert_eq!(retry_result, Ok(()));

    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 15);
    assert!(requests[2].starts_with("DELETE "));
    assert!(requests[7].starts_with("DELETE "));
    assert!(requests[8].contains(RUNNER_ID));
    assert!(requests[9].contains("wcompletion-runner"));
    assert!(requests[12].starts_with("DELETE "));
    Ok(())
}

#[tokio::test]
async fn retry_rejects_a_different_container_at_the_durable_name() -> Result<(), HostError> {
    let scratch = Scratch::open("replacement")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let stub = DockerStub::open(vec![
        response(404, r#"{"message":"missing"}"#),
        response(200, &container_json("replacement-id", "runner")),
    ])?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("test token");
    let context = Context {
        journal: &journal,
        docker: &stub.docker,
        transport: &mut transport,
        admin: &admin,
    };
    let result = run_cleanup(&context, &launch, claim).await;
    assert_eq!(result, Err(Failure::not_proven("container name identity")));
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    Ok(())
}

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

fn container_json(id: &str, role: &str) -> String {
    serde_json::json!({
        "Id": id,
        "Config": {
            "Labels": {
                "velnor.volume": WORKER,
                "velnor.worker": WORKER,
                "velnor.role": role,
            }
        }
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
        let task = tokio::spawn(serve(listener, responses));
        let socket = path.to_str().ok_or(HostError::Path)?;
        let docker = Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|_| HostError::Docker)?;
        Ok(Self {
            docker,
            path,
            task: Some(task),
        })
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

async fn serve(listener: UnixListener, responses: Vec<Response>) -> Result<Vec<String>, String> {
    let mut requests = Vec::with_capacity(responses.len());
    for response in responses {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        requests.push(read_request_line(&mut stream).await?);
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

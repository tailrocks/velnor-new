//! Admission decisions against worker volume state: uncertain
//! journals hold without settlement and post-delete absence
//! failures do not become cleanup proof.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::super::admission;
use crate::launch::Admit;
use crate::launch::harness::{Scratch, assigned_wait};
use velnor_runner_host::{EnsureError, IntentState, Journal, Outcome};

const TIMEOUT: Duration = Duration::from_secs(2);
const WORKER: &str = "wtransport";

fn volume_names() -> [(&'static str, &'static str); 3] {
    [
        ("wtransport", "socket"),
        ("wtransport-work", "work"),
        ("wtransport-docker", "dind-data"),
    ]
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

fn container_json(id: &str, worker: Option<&str>, role: Option<&str>) -> String {
    let labels = match (worker, role) {
        (Some(worker), Some(role)) => serde_json::json!({
            "velnor.worker": worker,
            "velnor.volume": worker,
            "velnor.role": role
        }),
        _ => serde_json::json!({}),
    };
    serde_json::json!({"Id": id, "Config": {"Labels": labels}}).to_string()
}

#[tokio::test]
async fn uncertain_volume_holds_without_remote_settlement() -> Result<(), String> {
    let scratch = Scratch::new("volume-post-delete").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let (row, fresh) = journal
        .begin_launch("offer-transport")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);
    journal
        .bind_worker_volume(row, WORKER)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;

    let stub = DockerStub::open(Vec::new())?;
    let decision = admission(&stub.docker, &journal, 1, 1, 0, &assigned_wait(1, 1)).await;
    let requests = stub.finish().await?;

    assert_eq!(decision, Ok(Admit::Hold));
    assert!(requests.is_empty());
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].docker_id, None);
    assert_eq!(rows[0].dind_id, None);
    assert_eq!(rows[0].worker_volume.as_deref(), Some(WORKER));
    assert!(!rows[0].cleanup_proven);
    Ok(())
}

#[tokio::test]
async fn done_post_delete_non_404_keeps_cleanup_unproven() -> Result<(), String> {
    let scratch = Scratch::new("volume-post-delete").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let (row, fresh) = journal
        .begin_launch("offer-transport")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);
    journal
        .bind_worker_volume(row, WORKER)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;

    let stub = DockerStub::open(pair_cleanup_responses())?;
    let decision = admission(&stub.docker, &journal, 1, 1, 0, &assigned_wait(1, 1)).await;
    let requests = stub.finish().await?;

    assert_eq!(
        decision,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "docker"
        })
    );
    assert_eq!(requests.len(), 18);
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Done);
    assert!(rows[0].docker_id.is_some());
    assert!(rows[0].dind_id.is_some());
    assert!(!rows[0].cleanup_proven);
    Ok(())
}

fn pair_cleanup_responses() -> Vec<Response> {
    let mut responses = vec![
        http(
            200,
            &container_json("runner-id", Some(WORKER), Some("runner")),
        ),
        http(200, &container_json("dind-id", Some(WORKER), Some("dind"))),
        http(200, r#"{"State":{"Running":false}}"#),
        http(
            200,
            &container_json("runner-id", Some(WORKER), Some("runner")),
        ),
        http(204, ""),
        http(404, r#"{"message":"missing"}"#),
        http(200, &container_json("dind-id", Some(WORKER), Some("dind"))),
        http(204, ""),
        http(404, r#"{"message":"missing"}"#),
    ];
    for (index, (name, role)) in volume_names().into_iter().enumerate() {
        responses.push(http(200, &volume_json(name, WORKER, role)));
        responses.push(http(204, ""));
        let status = if index == 2 { 500 } else { 404 };
        responses.push(http(status, r#"{"message":"not absent"}"#));
    }
    responses
}

struct Response {
    status: u16,
    body: String,
}

fn http(status: u16, body: &str) -> Response {
    Response {
        status,
        body: body.to_owned(),
    }
}

struct DockerStub {
    docker: Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<Vec<String>, String>>>,
}

impl DockerStub {
    fn open(responses: Vec<Response>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-worker-docker-{}-{number}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        let task = tokio::spawn(serve(listener, responses));
        let socket = path
            .to_str()
            .ok_or_else(|| "Docker socket path is not UTF-8".to_owned())?;
        let docker = Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            docker,
            path,
            task: Some(task),
        })
    }

    async fn finish(mut self) -> Result<Vec<String>, String> {
        let mut task = self
            .task
            .take()
            .ok_or_else(|| "Docker stub already stopped".to_owned())?;
        let Ok(result) = tokio::time::timeout(TIMEOUT, &mut task).await else {
            task.abort();
            return Err("Docker stub timed out".to_owned());
        };
        let requests = result.map_err(|error| error.to_string())??;
        std::fs::remove_file(&self.path).map_err(|error| error.to_string())?;
        Ok(requests)
    }
}

impl Drop for DockerStub {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let _kept = std::fs::remove_file(&self.path)
            .err()
            .map(|error| error.kind());
    }
}

async fn serve(listener: UnixListener, responses: Vec<Response>) -> Result<Vec<String>, String> {
    let mut requests = Vec::with_capacity(responses.len());
    for response in responses {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let request = read_request(&mut stream).await?;
        requests.push(request);
        send_response(&mut stream, response).await?;
    }
    Ok(requests)
}

async fn read_request(stream: &mut UnixStream) -> Result<String, String> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("Docker client closed before sending a request".to_owned());
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers =
                std::str::from_utf8(&request[..header_end]).map_err(|error| error.to_string())?;
            let body_len = content_length(headers)?;
            let request_end = header_end + 4 + body_len;
            if request.len() >= request_end {
                request.truncate(request_end);
                return String::from_utf8(request).map_err(|error| error.to_string());
            }
        }
    }
}

fn content_length(headers: &str) -> Result<usize, String> {
    for line in headers.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value
                .trim()
                .parse::<usize>()
                .map_err(|error| error.to_string());
        }
    }
    Ok(0)
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
        201 => "Created",
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

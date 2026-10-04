//! Docker inspect errors must stop capacity and reconcile decisions.

use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::launch::{gate, slot};
use crate::launch_harness::Scratch;
use crate::{EnsureError, IntentState, Journal};

const TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn non_not_found_inspect_error_blocks_admission_and_reconcile() -> Result<(), String> {
    let (scratch, journal) = journal("inspect-error").await?;
    let row_id = launch_row(&journal).await?;
    let before = journal.rows().await.map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![
        http(500, r#"{"message":"private runner-id detail"}"#),
        http(500, r#"{"message":"private runner-id detail"}"#),
    ])?;

    let busy = within(slot::busy(&journal, &stub.docker, 1), "capacity probe").await?;
    let reconcile = within(
        gate::reconcile_gate(&journal, &stub.docker),
        "reconcile probe",
    )
    .await?;
    stub.finish().await?;

    let expected = inspect_error(500);
    assert_eq!(busy, Err(expected));
    assert_eq!(reconcile, Err(expected));
    let errors = format!("{busy:?} {reconcile:?}");
    if errors.contains("runner-id") || errors.contains("private runner-id detail") {
        return Err("inspect error exposed Docker response data".to_owned());
    }
    let after = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(after, before);
    assert_eq!(after[0].id, row_id);
    assert_eq!(after[0].state, IntentState::Pending);
    assert_eq!(after[0].docker_id.as_deref(), Some("runner-id"));
    no_response_body_in_journal(&scratch.file())
}

#[tokio::test]
async fn docker_api_observations_preserve_only_known_running_states() -> Result<(), String> {
    let (scratch, journal) = journal("inspect-states").await?;
    launch_row(&journal).await?;
    let before = journal.rows().await.map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![
        http(404, r#"{"message":"missing"}"#),
        http(200, r#"{"State":{"Running":true}}"#),
        http(200, r#"{"State":{"Running":false}}"#),
        http(200, "{}"),
        http(200, r#"{"State":{}}"#),
        http(200, "{}"),
    ])?;

    for expected in [
        Ok(0),
        Ok(1),
        Ok(0),
        Err(inspect_error(200)),
        Err(inspect_error(200)),
    ] {
        let actual = within(
            slot::running_count(&journal, &stub.docker),
            "slot observation",
        )
        .await?;
        assert_eq!(actual, expected);
    }
    let gate = within(
        gate::reconcile_gate(&journal, &stub.docker),
        "reconcile probe",
    )
    .await?;
    stub.finish().await?;

    assert_eq!(gate, Err(inspect_error(200)));
    let after = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(after, before);
    assert_eq!(after[0].state, IntentState::Pending);
    no_response_body_in_journal(&scratch.file())
}

#[tokio::test]
async fn closed_docker_connection_is_not_treated_as_absent() -> Result<(), String> {
    let (scratch, journal) = journal("inspect-transport").await?;
    launch_row(&journal).await?;
    let before = journal.rows().await.map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![closed()])?;

    let actual = within(
        slot::running_count(&journal, &stub.docker),
        "slot observation",
    )
    .await?;
    stub.finish().await?;

    assert_eq!(actual, Err(inspect_error(0)));
    let after = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(after, before);
    no_response_body_in_journal(&scratch.file())
}

pub(super) async fn journal(label: &str) -> Result<(Scratch, Journal), String> {
    let scratch = Scratch::new(label).map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    Ok((scratch, journal))
}

pub(super) async fn launch_row(journal: &Journal) -> Result<i64, String> {
    launch_row_for_id(journal, "job", "runner-id").await
}

pub(super) async fn launch_row_for_id(
    journal: &Journal,
    subject: &str,
    docker_id: &str,
) -> Result<i64, String> {
    let row_id = journal
        .begin("launch", subject)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind(row_id, Some(docker_id), None)
        .await
        .map_err(|error| error.to_string())?;
    Ok(row_id)
}

pub(super) async fn within<F: Future>(future: F, label: &str) -> Result<F::Output, String> {
    tokio::time::timeout(TIMEOUT, future)
        .await
        .map_err(|_| format!("{label} timed out"))
}

pub(super) fn no_response_body_in_journal(path: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    if text.contains("private runner-id detail") {
        return Err("journal stored Docker response data".to_owned());
    }
    Ok(())
}

pub(super) const fn inspect_error(status: u16) -> EnsureError {
    EnsureError::Unexpected {
        status,
        step: "docker inspect",
    }
}

pub(super) struct DockerResponse {
    status: Option<u16>,
    body: String,
}

pub(super) struct DockerStub {
    pub(super) docker: Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

impl DockerStub {
    pub(super) fn open(responses: Vec<DockerResponse>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-inspect-{}-{number}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        let task = tokio::spawn(async move {
            for response in responses {
                send_response(&listener, &response).await?;
            }
            Ok(())
        });
        let socket = path
            .to_str()
            .ok_or_else(|| "Docker socket path is not UTF-8".to_owned())?;
        let docker = match Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION) {
            Ok(docker) => docker,
            Err(error) => {
                task.abort();
                let _removed = std::fs::remove_file(&path).err().map(|error| error.kind());
                return Err(error.to_string());
            }
        };
        Ok(Self {
            docker,
            path,
            task: Some(task),
        })
    }

    pub(super) async fn finish(mut self) -> Result<(), String> {
        let mut task = self
            .task
            .take()
            .ok_or_else(|| "Docker stub already stopped".to_owned())?;
        let served = if let Ok(result) = tokio::time::timeout(TIMEOUT, &mut task).await {
            result.map_err(|error| error.to_string())?
        } else {
            task.abort();
            return Err("Docker stub timed out waiting for requests".to_owned());
        };
        let removed = std::fs::remove_file(&self.path).map_err(|error| error.to_string());
        served?;
        removed
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

pub(super) fn http(status: u16, body: &str) -> DockerResponse {
    DockerResponse {
        status: Some(status),
        body: body.to_owned(),
    }
}

fn closed() -> DockerResponse {
    DockerResponse {
        status: None,
        body: String::new(),
    }
}

async fn send_response(listener: &UnixListener, response: &DockerResponse) -> Result<(), String> {
    let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
    read_request(&mut stream).await?;
    let Some(status) = response.status else {
        return Ok(());
    };
    let message = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        reason(status),
        response.body.len(),
        response.body
    );
    stream
        .write_all(message.as_bytes())
        .await
        .map_err(|error| error.to_string())
}

async fn read_request(stream: &mut UnixStream) -> Result<(), String> {
    let mut request = [0_u8; 2048];
    let read = stream
        .read(&mut request)
        .await
        .map_err(|error| error.to_string())?;
    if read == 0 {
        return Err("Docker client closed before sending a request".to_owned());
    }
    Ok(())
}

const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

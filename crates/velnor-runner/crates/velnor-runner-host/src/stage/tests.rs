//! Docker inspect responses must not turn uncertainty into absence.

use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::reconcile_worker;
use crate::error::HostError;
use crate::journal::LaunchIdentity;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn reconcile_treats_only_confirmed_not_found_as_absence() -> Result<(), String> {
    let stub = DockerStub::open(reconcile_responses(http(404, r#"{"message":"missing"}"#)))?;
    let observed = within(reconcile_worker(
        &stub.docker,
        &identity()?,
        None,
        None,
        None,
    ))
    .await?;
    stub.finish().await?;

    assert_eq!(observed, Ok(super::ObservedWorker::default()));
    Ok(())
}

#[tokio::test]
async fn reconcile_rejects_empty_missing_and_malformed_container_ids() -> Result<(), String> {
    let cases = [
        (
            http(200, r#"{"Id":"","Config":{"Labels":{}}}"#),
            HostError::Ownership,
        ),
        (
            http(200, r#"{"Config":{"Labels":{}}}"#),
            HostError::Ownership,
        ),
        (
            http(200, r#"{"Id":"not-an-id","Config":{"Labels":{}}}"#),
            HostError::Ownership,
        ),
        (http(200, "not-json"), HostError::Docker),
    ];

    for (response, expected) in cases {
        let stub = DockerStub::open(reconcile_responses(response))?;
        let result = within(reconcile_worker(
            &stub.docker,
            &identity()?,
            None,
            None,
            None,
        ))
        .await?;
        stub.finish().await?;
        assert_eq!(result, Err(expected));
    }
    Ok(())
}

#[tokio::test]
async fn reconcile_keeps_non_not_found_docker_errors_as_errors() -> Result<(), String> {
    for status in [401, 500, 503] {
        let message = format!(r#"{{"message":"private {status} detail"}}"#);
        let stub = DockerStub::open(reconcile_responses(http(status, &message)))?;
        let result = within(reconcile_worker(
            &stub.docker,
            &identity()?,
            None,
            None,
            None,
        ))
        .await?;
        stub.finish().await?;
        assert_eq!(result, Err(HostError::Docker));
        if format!("{result:?}").contains("private") {
            return Err("Docker response details escaped into the host error".to_owned());
        }
    }
    Ok(())
}

#[tokio::test]
async fn reconcile_keeps_closed_connections_as_errors() -> Result<(), String> {
    let stub = DockerStub::open(reconcile_responses(closed()))?;
    let result = within(reconcile_worker(
        &stub.docker,
        &identity()?,
        None,
        None,
        None,
    ))
    .await?;
    stub.finish().await?;

    assert_eq!(result, Err(HostError::Docker));
    Ok(())
}

#[tokio::test]
async fn reconcile_keeps_inspect_timeouts_uncertain() -> Result<(), String> {
    let stub = DockerStub::open(reconcile_responses(delayed_close(
        CLIENT_TIMEOUT + Duration::from_secs(1),
    )))?;
    let result = tokio::time::timeout(
        CLIENT_TIMEOUT + Duration::from_secs(3),
        reconcile_worker(&stub.docker, &identity()?, None, None, None),
    )
    .await
    .map_err(|_| "reconciliation did not respect its Docker inspect timeout".to_owned())?;
    stub.finish().await?;

    assert_eq!(result, Err(HostError::DockerTimeout));
    Ok(())
}

fn identity() -> Result<LaunchIdentity, String> {
    LaunchIdentity::new(
        "11111111111111111111111111111111",
        1,
        "22222222222222222222222222222222",
        "engine_identity",
    )
    .map_err(|error| error.to_string())
}

fn reconcile_responses(inspect_runner: Response) -> Vec<Response> {
    vec![
        http(200, r#"{"ID":"engine_identity"}"#),
        http(200, r#"{"ID":"engine_identity"}"#),
        http(200, "[]"),
        http(404, r#"{"message":"missing DinD"}"#),
        inspect_runner,
    ]
}

struct Response {
    status: Option<u16>,
    body: String,
    delay: Duration,
}

struct DockerStub {
    docker: Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

impl DockerStub {
    fn open(responses: Vec<Response>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-stage-inspect-{}-{number}.sock",
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

    async fn finish(mut self) -> Result<(), String> {
        let mut task = self
            .task
            .take()
            .ok_or_else(|| "Docker stub already stopped".to_owned())?;
        let served =
            match tokio::time::timeout(CLIENT_TIMEOUT + Duration::from_secs(3), &mut task).await {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => {
                    task.abort();
                    Err("Docker stub timed out waiting for requests".to_owned())
                }
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

fn http(status: u16, body: &str) -> Response {
    Response {
        status: Some(status),
        body: body.to_owned(),
        delay: Duration::ZERO,
    }
}

fn closed() -> Response {
    Response {
        status: None,
        body: String::new(),
        delay: Duration::ZERO,
    }
}

fn delayed_close(delay: Duration) -> Response {
    Response {
        status: None,
        body: String::new(),
        delay,
    }
}

async fn send_response(listener: &UnixListener, response: &Response) -> Result<(), String> {
    let (mut stream, _) = tokio::time::timeout(REQUEST_TIMEOUT, listener.accept())
        .await
        .map_err(|_| "Docker stub accept timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    read_request(&mut stream).await?;
    tokio::time::sleep(response.delay).await;
    let Some(status) = response.status else {
        return Ok(());
    };
    let message = format!(
        concat!(
            "HTTP/1.1 {} {}\r\n",
            "Content-Type: application/json\r\n",
            "Content-Length: {}\r\n",
            "Connection: close\r\n\r\n{}"
        ),
        status,
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

async fn within<F: Future>(future: F) -> Result<F::Output, String> {
    tokio::time::timeout(CLIENT_TIMEOUT + Duration::from_secs(3), future)
        .await
        .map_err(|_| "Docker reconciliation timed out".to_owned())
}

const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

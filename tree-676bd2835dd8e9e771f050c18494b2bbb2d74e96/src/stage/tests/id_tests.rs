use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::super::{PairEngine, remove_recorded};
use crate::docker_spec::DeleteDecision;
use crate::error::HostError;

const TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn inspect_only_treats_not_found_as_absent_and_requires_id() -> Result<(), String> {
    let stub = DockerStub::open(vec![
        http(404, r#"{"message":"private detail"}"#),
        http(200, r#"{"Id":"owned-id"}"#),
        http(200, "{}"),
        http(200, r#"{"Id":""}"#),
        http(200, "not-json"),
        http(500, r#"{"message":"private detail"}"#),
        closed(),
    ])?;

    assert_eq!(within(stub.docker.id_for_name("runner")).await?, Ok(None));
    assert_eq!(
        within(stub.docker.id_for_name("runner")).await?,
        Ok(Some("owned-id".to_owned()))
    );
    for _ in 0..5 {
        let result = within(stub.docker.id_for_name("runner")).await?;
        assert_eq!(result, Err(HostError::Docker));
        assert!(!format!("{result:?}").contains("private detail"));
    }
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 7);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    Ok(())
}

#[tokio::test]
async fn cleanup_fails_closed_for_uncertain_identity_and_empty_name() -> Result<(), String> {
    let stub = DockerStub::open(vec![
        http(500, r#"{"message":"private detail"}"#),
        http(200, "not-json"),
        http(200, "{}"),
        http(200, r#"{"Id":""}"#),
        closed(),
    ])?;

    assert_eq!(
        within(remove_recorded(&stub.docker, "owned-id", "")).await?,
        Err(HostError::Docker)
    );
    for _ in 0..5 {
        let result = within(remove_recorded(&stub.docker, "owned-id", "runner")).await?;
        assert_eq!(result, Err(HostError::Docker));
        assert!(!format!("{result:?}").contains("private detail"));
    }
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 5);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    Ok(())
}

#[tokio::test]
async fn cleanup_removes_only_verified_owned_identity() -> Result<(), String> {
    let stub = DockerStub::open(vec![
        http(404, r#"{"message":"missing"}"#),
        http(200, r#"{"Id":"owned-id"}"#),
        http(204, ""),
        http(200, r#"{"Id":"foreign-id"}"#),
    ])?;

    assert_eq!(
        within(remove_recorded(&stub.docker, "owned-id", "runner")).await?,
        Ok(DeleteDecision::NotDeleted)
    );
    assert_eq!(
        within(remove_recorded(&stub.docker, "owned-id", "runner")).await?,
        Ok(DeleteDecision::Delete)
    );
    assert_eq!(
        within(remove_recorded(&stub.docker, "owned-id", "runner")).await?,
        Ok(DeleteDecision::KeepForeign)
    );
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 4);
    assert!(requests[0].starts_with("GET "));
    assert!(requests[1].starts_with("GET "));
    assert!(requests[2].starts_with("DELETE "));
    assert!(requests[2].contains("/owned-id"));
    assert!(requests[3].starts_with("GET "));
    Ok(())
}

struct Response {
    status: Option<u16>,
    body: String,
}

struct DockerStub {
    docker: Docker,
    path: PathBuf,
    requests: Arc<Mutex<Vec<String>>>,
    task: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

impl DockerStub {
    fn open(responses: Vec<Response>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-stage-id-{}-{number}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let task_requests = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            for response in responses {
                send_response(&listener, &task_requests, &response).await?;
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
            requests,
            task: Some(task),
        })
    }

    async fn finish(mut self) -> Result<Vec<String>, String> {
        let mut task = self
            .task
            .take()
            .ok_or_else(|| "Docker stub already stopped".to_owned())?;
        let served = match tokio::time::timeout(TIMEOUT, &mut task).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => {
                task.abort();
                Err("Docker stub timed out waiting for requests".to_owned())
            }
        };
        let removed = std::fs::remove_file(&self.path).map_err(|error| error.to_string());
        served?;
        removed?;
        let requests = self
            .requests
            .lock()
            .map(|requests| requests.clone())
            .map_err(|_| "Docker request log is poisoned".to_owned())?;
        Ok(requests)
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
    }
}

fn closed() -> Response {
    Response {
        status: None,
        body: String::new(),
    }
}

async fn send_response(
    listener: &UnixListener,
    requests: &Mutex<Vec<String>>,
    response: &Response,
) -> Result<(), String> {
    let (mut stream, _) = tokio::time::timeout(TIMEOUT, listener.accept())
        .await
        .map_err(|_| "Docker server accept timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    record_request(&mut stream, requests).await?;
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

async fn record_request(
    stream: &mut UnixStream,
    requests: &Mutex<Vec<String>>,
) -> Result<(), String> {
    let mut bytes = [0_u8; 2048];
    let count = stream
        .read(&mut bytes)
        .await
        .map_err(|error| error.to_string())?;
    if count == 0 {
        return Err("Docker client closed before sending a request".to_owned());
    }
    let request = String::from_utf8_lossy(&bytes[..count]);
    let line = request
        .lines()
        .next()
        .ok_or_else(|| "Docker request line missing".to_owned())?
        .to_owned();
    requests
        .lock()
        .map_err(|_| "Docker request log is poisoned".to_owned())?
        .push(line);
    Ok(())
}

async fn within<F: Future>(future: F) -> Result<F::Output, String> {
    tokio::time::timeout(TIMEOUT, future)
        .await
        .map_err(|_| "Docker request timed out".to_owned())
}

const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

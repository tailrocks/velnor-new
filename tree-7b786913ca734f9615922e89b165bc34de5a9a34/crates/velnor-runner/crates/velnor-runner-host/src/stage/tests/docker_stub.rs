//! Bounded Docker HTTP stub for stage reconciliation tests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::journal::LaunchIdentity;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct Response {
    status: Option<u16>,
    body: String,
    delay: Duration,
    target: RequestTarget,
}

struct RequestTarget {
    method: &'static str,
    path: String,
    query_required: bool,
}

impl RequestTarget {
    fn get(path: &str, query_required: bool) -> Self {
        Self {
            method: "GET",
            path: path.to_owned(),
            query_required,
        }
    }
}

pub(super) struct DockerStub {
    pub(super) docker: Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

impl DockerStub {
    pub(super) fn open(responses: Vec<Response>) -> Result<Self, String> {
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

    pub(super) async fn finish(mut self) -> Result<(), String> {
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

impl Response {
    fn without_body(target: RequestTarget) -> Self {
        Self {
            status: None,
            body: String::new(),
            delay: Duration::ZERO,
            target,
        }
    }

    fn delayed_without_body(target: RequestTarget, delay: Duration) -> Self {
        Self {
            status: None,
            body: String::new(),
            delay,
            target,
        }
    }
}

pub(super) fn reconcile_responses(
    identity: &LaunchIdentity,
    inspect_runner: Response,
) -> Vec<Response> {
    vec![
        http(
            200,
            r#"{"ID":"engine_identity"}"#,
            RequestTarget::get("/info", false),
        ),
        http(
            200,
            r#"{"ID":"engine_identity"}"#,
            RequestTarget::get("/info", false),
        ),
        http(200, "[]", RequestTarget::get("/containers/json", true)),
        inspect_response(404, r#"{"message":"missing DinD"}"#, identity, "dind"),
        inspect_runner,
    ]
}

pub(super) fn inspect_response(
    status: u16,
    body: &str,
    identity: &LaunchIdentity,
    role: &str,
) -> Response {
    let name = crate::worker::container_name(identity, role);
    http(
        status,
        body,
        RequestTarget::get(&format!("/containers/{name}/json"), false),
    )
}

pub(super) fn inspect_response_without_body(identity: &LaunchIdentity, role: &str) -> Response {
    let name = crate::worker::container_name(identity, role);
    Response::without_body(RequestTarget::get(
        &format!("/containers/{name}/json"),
        false,
    ))
}

pub(super) fn delayed_inspect_close(
    identity: &LaunchIdentity,
    role: &str,
    delay: Duration,
) -> Response {
    let name = crate::worker::container_name(identity, role);
    Response::delayed_without_body(
        RequestTarget::get(&format!("/containers/{name}/json"), false),
        delay,
    )
}

fn http(status: u16, body: &str, target: RequestTarget) -> Response {
    Response {
        status: Some(status),
        body: body.to_owned(),
        delay: Duration::ZERO,
        target,
    }
}

async fn send_response(listener: &UnixListener, response: &Response) -> Result<(), String> {
    let (mut stream, _) = tokio::time::timeout(REQUEST_TIMEOUT, listener.accept())
        .await
        .map_err(|_| "Docker stub accept timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    tokio::time::timeout(REQUEST_TIMEOUT, read_request(&mut stream, &response.target))
        .await
        .map_err(|_| "Docker request line timed out".to_owned())??;
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

async fn read_request(stream: &mut UnixStream, expected: &RequestTarget) -> Result<(), String> {
    const MAX_REQUEST_LINE: usize = 2048;
    let mut request = Vec::with_capacity(128);
    let mut byte = [0_u8; 1];
    while request.len() < MAX_REQUEST_LINE {
        let read = stream
            .read(&mut byte)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("Docker client closed before sending a request line".to_owned());
        }
        request.push(byte[0]);
        if request.ends_with(b"\r\n") {
            break;
        }
    }
    if !request.ends_with(b"\r\n") {
        return Err("Docker request line exceeded its size bound".to_owned());
    }
    let line = std::str::from_utf8(&request[..request.len() - 2])
        .map_err(|_| "Docker request line is not UTF-8".to_owned())?;
    let mut fields = line.split_whitespace();
    let method = fields
        .next()
        .ok_or_else(|| "Docker request method is missing".to_owned())?;
    let actual_path = fields
        .next()
        .ok_or_else(|| "Docker request path is missing".to_owned())?;
    let protocol = fields
        .next()
        .ok_or_else(|| "Docker request protocol is missing".to_owned())?;
    if fields.next().is_some() || method != expected.method || protocol != "HTTP/1.1" {
        return Err("Docker request line does not match the scripted request".to_owned());
    }
    verify_versioned_path(actual_path, expected)
}

fn verify_versioned_path(actual: &str, expected: &RequestTarget) -> Result<(), String> {
    let versioned = actual
        .strip_prefix("/v")
        .ok_or_else(|| "Docker request path has no API version".to_owned())?;
    let slash = versioned
        .find('/')
        .ok_or_else(|| "Docker request API path is missing".to_owned())?;
    let version = &versioned[..slash];
    let Some((major, minor)) = version.split_once('.') else {
        return Err("Docker API version is malformed".to_owned());
    };
    if major.is_empty()
        || minor.is_empty()
        || !major.bytes().all(|byte| byte.is_ascii_digit())
        || !minor.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("Docker API version is malformed".to_owned());
    }
    let path = &versioned[slash..];
    if expected.query_required {
        match path.split_once('?') {
            Some((actual_path, query)) if actual_path == expected.path && !query.is_empty() => {
                Ok(())
            }
            _ => Err("Docker request path does not match the scripted request".to_owned()),
        }
    } else if path == expected.path {
        Ok(())
    } else {
        Err("Docker request path does not match the scripted request".to_owned())
    }
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

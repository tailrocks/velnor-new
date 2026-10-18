use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bollard::Docker;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;

use super::super::support::{self, DOCKER_ROOT};

const TIMEOUT: Duration = Duration::from_secs(10);
const HTTP_LIMIT: usize = 128 * 1024;
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

pub(super) struct CapturedRequest {
    pub(super) method: String,
    pub(super) target: String,
    pub(super) body: Vec<u8>,
}

pub(super) struct CapturedApi {
    pub(super) requests: Vec<CapturedRequest>,
    pub(super) jit_input: Vec<u8>,
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
        let path = std::env::temp_dir().join(format!("vp-{}.sock", uuid::Uuid::new_v4()));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let jit_input = Arc::new(Mutex::new(Vec::new()));
        let (stop, stopped) = oneshot::channel();
        let server_requests = Arc::clone(&requests);
        let server_jit = Arc::clone(&jit_input);
        let task = tokio::spawn(async move {
            serve(listener, images, server_requests, server_jit, stopped).await
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
        let served = tokio::time::timeout(TIMEOUT, &mut task)
            .await
            .map_err(|_| "Docker API test server timed out".to_owned())?
            .map_err(|error| error.to_string())?;
        served?;
        std::fs::remove_file(&self.path).map_err(|error| error.to_string())?;
        Ok(CapturedApi {
            requests: take(&self.requests, "Docker request list is poisoned")?,
            jit_input: take(&self.jit_input, "JIT input is poisoned")?,
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

async fn serve(
    listener: UnixListener,
    images: crate::launch::VerifiedWorkerImages,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    jit_input: Arc<Mutex<Vec<u8>>>,
    mut stop: oneshot::Receiver<()>,
) -> Result<(), String> {
    loop {
        let accepted = tokio::select! {
            _ = &mut stop => return Ok(()),
            accepted = tokio::time::timeout(TIMEOUT, listener.accept()) => accepted,
        };
        let (mut stream, _) = accepted
            .map_err(|_| "Docker API request timed out".to_owned())?
            .map_err(|error| error.to_string())?;
        let request = read_request(&mut stream).await?;
        let is_attach = request.method == "POST" && route(&request).ends_with("/attach");
        requests
            .lock()
            .map_err(|_| "Docker request list is poisoned".to_owned())?
            .push(request);
        if is_attach {
            record_jit_input(&mut stream, &jit_input).await?;
            continue;
        }
        let response = {
            let requests = requests
                .lock()
                .map_err(|_| "Docker request list is poisoned".to_owned())?;
            let request = requests
                .last()
                .ok_or_else(|| "Docker request log is empty".to_owned())?;
            answer(&images, request)?
        };
        write_response(&mut stream, response.0, &response.1).await?;
    }
}

async fn record_jit_input(
    stream: &mut UnixStream,
    jit_input: &Mutex<Vec<u8>>,
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
        .map_err(|_| "JIT input is poisoned".to_owned())?
        .extend(payload);
    Ok(())
}

async fn read_request(stream: &mut UnixStream) -> Result<CapturedRequest, String> {
    let mut bytes = Vec::new();
    let header_end = read_headers(stream, &mut bytes).await?;
    let header = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| "Docker request headers are not UTF-8".to_owned())?;
    let mut lines = header.lines();
    let mut request = lines
        .next()
        .ok_or_else(|| "Docker request line is absent".to_owned())?
        .split_whitespace();
    let method = request
        .next()
        .ok_or_else(|| "Docker request method is absent".to_owned())?
        .to_owned();
    let target = request
        .next()
        .ok_or_else(|| "Docker request path is absent".to_owned())?
        .to_owned();
    let length = content_length(lines)?;
    if header_end + length > HTTP_LIMIT {
        return Err("Docker API request exceeds the test limit".to_owned());
    }
    read_body(stream, &mut bytes, header_end + length).await?;
    Ok(CapturedRequest {
        method,
        target,
        body: bytes[header_end..header_end + length].to_vec(),
    })
}

async fn read_headers(stream: &mut UnixStream, bytes: &mut Vec<u8>) -> Result<usize, String> {
    loop {
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            return Ok(index + 4);
        }
        if bytes.len() >= HTTP_LIMIT {
            return Err("Docker API headers exceed the test limit".to_owned());
        }
        let mut chunk = [0_u8; 2048];
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("Docker closed before sending request headers".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

async fn read_body(
    stream: &mut UnixStream,
    bytes: &mut Vec<u8>,
    full_length: usize,
) -> Result<(), String> {
    while bytes.len() < full_length {
        let mut chunk = [0_u8; 2048];
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("Docker closed before sending the request body".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(())
}

fn content_length<'a>(lines: impl Iterator<Item = &'a str>) -> Result<usize, String> {
    for line in lines {
        if let Some((name, value)) = line.trim_end_matches('\r').split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            return value
                .trim()
                .parse()
                .map_err(|_| "Docker content length is malformed".to_owned());
        }
    }
    Ok(0)
}

fn answer(
    images: &crate::launch::VerifiedWorkerImages,
    request: &CapturedRequest,
) -> Result<(u16, Vec<u8>), String> {
    let route = route(request);
    match (request.method.as_str(), route) {
        ("GET", "/info") => Ok((
            200,
            support::engine_info(
                images.engine_id(),
                DOCKER_ROOT,
                images.daemon_architecture(),
            )
            .into_bytes(),
        )),
        ("GET", path) if path.starts_with("/images/") => {
            Ok((200, image_inspection(images, path)?.into_bytes()))
        }
        ("GET", path) if path.starts_with("/volumes/") => {
            Ok((404, br#"{"message":"missing"}"#.to_vec()))
        }
        ("POST", "/volumes/create") => create_volume_response(&request.body),
        ("POST", "/containers/create") => create_container_response(images, &request.body),
        ("POST", path) if path.ends_with("/start") => Ok((204, Vec::new())),
        _ => Err("Docker API call is outside the scoped pair test".to_owned()),
    }
}

fn image_inspection(
    images: &crate::launch::VerifiedWorkerImages,
    path: &str,
) -> Result<String, String> {
    let normalized = path.replace("%3A", ":").replace("%3a", ":");
    if normalized.contains(images.runner().runtime_id()) {
        Ok(support::image_inspect(
            images,
            "velnor-runner",
            images.runner(),
        ))
    } else if normalized.contains(images.dind().runtime_id()) {
        Ok(support::image_inspect(images, "velnor-dind", images.dind()))
    } else {
        Err("Docker inspected an image outside the verified bundle".to_owned())
    }
}

fn create_volume_response(body: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let value: Value = serde_json::from_slice(body).map_err(|error| error.to_string())?;
    let name = value["Name"]
        .as_str()
        .ok_or_else(|| "Docker volume name is absent".to_owned())?;
    let response = json!({
        "Name": name,
        "Driver": "local",
        "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"),
        "Labels": value["Labels"],
        "Options": {},
        "Scope": "local"
    });
    Ok((201, response.to_string().into_bytes()))
}

fn create_container_response(
    images: &crate::launch::VerifiedWorkerImages,
    body: &[u8],
) -> Result<(u16, Vec<u8>), String> {
    let value: Value = serde_json::from_slice(body).map_err(|error| error.to_string())?;
    let image = value["Image"]
        .as_str()
        .ok_or_else(|| "Docker container image is absent".to_owned())?;
    let id = if image == images.dind().runtime_id() {
        DIND_ID
    } else if image == images.runner().runtime_id() {
        RUNNER_ID
    } else {
        return Err("Docker create used an image outside the verified bundle".to_owned());
    };
    Ok((
        201,
        json!({"Id": id, "Warnings": []}).to_string().into_bytes(),
    ))
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

fn take<T: Default>(value: &Mutex<T>, message: &'static str) -> Result<T, String> {
    value
        .lock()
        .map_err(|_| message.to_owned())
        .map(|mut value| std::mem::take(&mut *value))
}

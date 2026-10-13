use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::{WORKER, container_json, volume_json, volume_names};
use crate::worker::{create_named_volumes, remove_worker_volumes, worker_id_for_name};
use crate::{HostError, dind_create, dind_create_for_profile};
use velnor_runner_docker_spec::{resolve_runner_profile, runner_plan_for_profile};

fn profile_volume_names() -> [(&'static str, &'static str); 6] {
    let mut names = super::official_volume_names().to_vec();
    names.extend([
        ("wtransport-home", "home-state"),
        ("wtransport-tmp", "runner-temp"),
    ]);
    names.try_into().expect("the profile has six owned volumes")
}

#[tokio::test]
async fn only_exactly_owned_volumes_are_removed() -> Result<(), String> {
    let foreign = DockerStub::open(vec![http(
        200,
        &volume_json("wtransport", "another-worker", "socket"),
    )])?;
    let refused = remove_worker_volumes(&foreign.docker, WORKER).await;
    let foreign_requests = foreign.finish().await?;
    assert_eq!(refused, Ok(false));
    assert_eq!(foreign_requests.len(), 1);
    assert!(foreign_requests[0].starts_with("GET "));

    let mut responses = Vec::new();
    for (name, role) in profile_volume_names() {
        responses.push(http(200, &volume_json(name, WORKER, role)));
        responses.push(http(204, ""));
        responses.push(http(404, r#"{"message":"missing"}"#));
    }
    let owned = DockerStub::open(responses)?;
    let removed = remove_worker_volumes(&owned.docker, WORKER).await;
    let requests = owned.finish().await?;

    assert_eq!(removed, Ok(true));
    assert_eq!(requests.len(), 18);
    assert_eq!(
        requests
            .iter()
            .filter(|line| line.starts_with("DELETE "))
            .count(),
        6
    );
    Ok(())
}

#[tokio::test]
async fn created_volumes_have_exact_worker_and_role_labels() -> Result<(), String> {
    let expected = volume_names();
    let responses = expected
        .iter()
        .map(|(name, role)| http(201, &volume_json(name, WORKER, role)))
        .collect();
    let stub = DockerStub::open(responses)?;
    let plan = dind_create(WORKER).map_err(|error| error.to_string())?;
    let created = create_named_volumes(&stub.docker, WORKER, &plan.mounts).await;
    let requests = stub.finish().await?;

    assert_eq!(created, Ok(()));
    assert_eq!(requests.len(), expected.len());
    for (request, (name, role)) in requests.iter().zip(expected) {
        let body = request_body(request)?;
        let value: serde_json::Value =
            serde_json::from_str(body).map_err(|error| error.to_string())?;
        assert_eq!(
            value.get("Name").and_then(serde_json::Value::as_str),
            Some(name)
        );
        assert_eq!(
            value.get("Labels"),
            Some(&serde_json::json!({"velnor.worker": WORKER, "velnor.role": role}))
        );
    }
    Ok(())
}

#[test]
fn legacy_volumes_keep_the_pre_rofs_catalog() -> Result<(), String> {
    let plan = dind_create(WORKER).map_err(|error| error.to_string())?;
    let catalog = super::super::volumes_for_mounts(WORKER, &plan.mounts)
        .map_err(|error| error.to_string())?;
    let observed = catalog
        .iter()
        .map(|volume| (volume.name.as_str(), volume.role))
        .collect::<Vec<_>>();

    assert_eq!(observed, volume_names());
    assert!(
        observed
            .iter()
            .all(|(_, role)| !matches!(*role, "externals" | "home-state" | "runner-temp"))
    );
    Ok(())
}

#[tokio::test]
async fn official_profile_creates_the_shared_externals_volume() -> Result<(), String> {
    let expected = profile_volume_names();
    let responses = expected
        .iter()
        .map(|(name, role)| http(201, &volume_json(name, WORKER, role)))
        .collect();
    let stub = DockerStub::open(responses)?;
    let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")
        .map_err(|error| error.to_string())?;
    let plan = dind_create_for_profile(WORKER, &profile).map_err(|error| error.to_string())?;
    assert_eq!(plan.mounts[0].source, format!("volume:{WORKER}"));
    assert_eq!(plan.mounts[0].target, "/var/run");
    assert_eq!(
        plan.cmd.get(1).map(String::as_str),
        Some("--host=unix:///var/run/docker.sock")
    );
    assert_eq!(plan.mounts[1].target, "/home/runner/_work");
    assert_eq!(plan.mounts[2].target, "/home/runner/externals");
    let mut mounts = runner_plan_for_profile(WORKER, &profile)
        .map_err(|error| error.to_string())?
        .mounts;
    mounts.extend(plan.mounts);
    let created = create_named_volumes(&stub.docker, WORKER, &mounts).await;
    let requests = stub.finish().await?;

    assert_eq!(created, Ok(()));
    assert_eq!(requests.len(), expected.len());
    let external = requests
        .iter()
        .find(|request| request.contains("wtransport-externals"))
        .ok_or_else(|| "missing externals volume request".to_owned())?;
    let body = request_body(external)?;
    let value: serde_json::Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    assert_eq!(
        value.get("Labels"),
        Some(&serde_json::json!({"velnor.worker": WORKER, "velnor.role": "externals"}))
    );
    Ok(())
}

#[tokio::test]
async fn volume_delete_error_fails_closed() -> Result<(), String> {
    let stub = DockerStub::open(vec![
        http(200, &volume_json("wtransport", WORKER, "socket")),
        http(500, r#"{"message":"busy"}"#),
    ])?;
    let result = remove_worker_volumes(&stub.docker, WORKER).await;
    let requests = stub.finish().await?;

    assert_eq!(result, Err(HostError::Docker));
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("DELETE "));
    Ok(())
}

#[tokio::test]
async fn container_identity_requires_id_and_exact_labels() -> Result<(), String> {
    let stub = DockerStub::open(vec![
        http(
            200,
            &container_json("runner-id", Some(WORKER), Some("runner")),
        ),
        http(
            200,
            r#"{"Config":{"Labels":{"velnor.worker":"wtransport","velnor.role":"runner","velnor.volume":"wtransport"}}}"#,
        ),
        http(200, r#"{"Id":"runner-id"}"#),
        http(
            200,
            &container_json("runner-id", Some("other"), Some("runner")),
        ),
        http(404, r#"{"message":"missing"}"#),
    ])?;

    assert_eq!(
        worker_id_for_name(&stub.docker, "wtransport-runner", WORKER, "runner").await,
        Ok(Some("runner-id".to_owned()))
    );
    for _ in 0..3 {
        assert_eq!(
            worker_id_for_name(&stub.docker, "wtransport-runner", WORKER, "runner").await,
            Err(HostError::Docker)
        );
    }
    assert_eq!(
        worker_id_for_name(&stub.docker, "wtransport-runner", WORKER, "runner").await,
        Ok(None)
    );
    assert_eq!(stub.finish().await?.len(), 5);
    Ok(())
}

pub(super) struct Response {
    status: u16,
    body: String,
}

pub(super) fn http(status: u16, body: &str) -> Response {
    Response {
        status,
        body: body.to_owned(),
    }
}

pub(super) struct DockerStub {
    pub(super) docker: Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<Vec<String>, String>>>,
}

impl DockerStub {
    pub(super) fn open(responses: Vec<Response>) -> Result<Self, String> {
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

    pub(super) async fn finish(mut self) -> Result<Vec<String>, String> {
        let mut task = self
            .task
            .take()
            .ok_or_else(|| "Docker stub already stopped".to_owned())?;
        let Ok(result) = tokio::time::timeout(super::TIMEOUT, &mut task).await else {
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

fn request_body(request: &str) -> Result<&str, String> {
    request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .ok_or_else(|| "Docker request omitted HTTP headers".to_owned())
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

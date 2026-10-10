use std::path::PathBuf;
use std::time::Duration;

use bollard::{ClientVersion, Docker, models::ContainerConfig};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout};

use super::load_and_inspect;
use crate::artifact_admission::archive::ArchiveIdentity;
use crate::artifact_admission::manifest::{
    ARCHIVE_FORMAT, DEFAULT_ENV, IMAGE_ENTRYPOINT, IMAGE_TAG, IMAGE_USER, IMAGE_WORKDIR, PLATFORM,
    ProbeManifest, SOURCE_LABEL,
};
use crate::artifact_admission::release::VerifiedRelease;
use crate::error::HostError;

const ENGINE_ID: &str = "engine-docker-29-4";
const DOCKER_ROOT: &str = "/var/lib/docker";
const CONTAINERD_MANIFEST: &str =
    "sha256:055b3124b01a1b4b5b1c06fcd8b2b27859948c0c9812129121157b8ed78fed48";
const CLASSIC_CONFIG: &str =
    "sha256:2fb80254177669698c443a8414897d0afcf40f1eac64456cb149339e599c66bb";
const OCI_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const PLATFORM_QUERY: &str =
    "platform=%7B%22architecture%22%3A%22amd64%22%2C%22os%22%3A%22linux%22%7D";

struct Reply {
    status: u16,
    body: String,
}

enum PostInspectInfo {
    Stable,
    Changed(String),
    None,
}

struct TestDaemon {
    docker: Docker,
    path: PathBuf,
    server: Option<JoinHandle<Result<Vec<String>, HostError>>>,
    completion: Option<oneshot::Sender<()>>,
    finished: bool,
}

impl TestDaemon {
    fn open(replies: Vec<Reply>, version: ClientVersion) -> Result<Self, HostError> {
        let path = socket_path();
        let socket = path.to_str().ok_or(HostError::Path)?;
        let listener = UnixListener::bind(&path).map_err(|_| HostError::Docker)?;
        let Ok(docker) = Docker::connect_with_unix(socket, 120, &version) else {
            remove_socket(&path)?;
            return Err(HostError::Docker);
        };
        let (completion, wait) = oneshot::channel();
        let server = tokio::spawn(serve(listener, replies, wait));
        Ok(Self {
            docker,
            path,
            server: Some(server),
            completion: Some(completion),
            finished: false,
        })
    }

    async fn finish(mut self) -> Result<Vec<String>, HostError> {
        self.completion
            .take()
            .ok_or(HostError::Docker)?
            .send(())
            .map_err(|()| HostError::Docker)?;
        let server = self.server.take().ok_or(HostError::Docker)?;
        let requests = timeout(Duration::from_secs(2), server)
            .await
            .map_err(|_| HostError::DockerTimeout)?
            .map_err(|_| HostError::Docker)??;
        remove_socket(&self.path)?;
        self.finished = true;
        Ok(requests)
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.abort();
        }
        if !self.finished
            && let Err(error) = std::fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("test Docker socket cleanup failed: {error}");
        }
    }
}

#[tokio::test]
async fn recorded_docker_29_4_containerd_and_classic_profiles_use_explicit_platform()
-> Result<(), HostError> {
    for (response, expected_id) in [
        (containerd_image()?, CONTAINERD_MANIFEST),
        (classic_image()?, CLASSIC_CONFIG),
    ] {
        let (result, requests) = run_existing_image(response, PostInspectInfo::Stable).await?;
        assert!(
            result.is_ok(),
            "inspect failed: {:?}; {requests:?}",
            result.as_ref().err()
        );
        let runtime = result?;
        assert_eq!(runtime.runtime_id, expected_id);
        assert_eq!(requests.len(), 3, "{requests:?}");
        assert_inspect_query(&requests[1]);
        assert_no_import(&requests);
    }
    Ok(())
}

#[tokio::test]
async fn rejects_default_index_identity_without_importing_the_existing_image()
-> Result<(), HostError> {
    let result = run_existing_image(index_image()?, PostInspectInfo::None).await?;
    assert!(matches!(result.0, Err(HostError::Identity)));
    assert_eq!(result.1.len(), 2);
    assert_inspect_query(&result.1[1]);
    assert_no_import(&result.1);
    Ok(())
}

async fn run_existing_image(
    image: String,
    after_inspect: PostInspectInfo,
) -> Result<(Result<super::RuntimeImage, HostError>, Vec<String>), HostError> {
    let mut replies = vec![
        Reply {
            status: 200,
            body: info_response(ENGINE_ID, DOCKER_ROOT),
        },
        Reply {
            status: 200,
            body: image,
        },
    ];
    match after_inspect {
        PostInspectInfo::Stable => replies.push(Reply {
            status: 200,
            body: info_response(ENGINE_ID, DOCKER_ROOT),
        }),
        PostInspectInfo::Changed(body) => replies.push(Reply { status: 200, body }),
        PostInspectInfo::None => {}
    }
    let daemon = TestDaemon::open(replies, *bollard::API_DEFAULT_VERSION)?;
    let result = load_and_inspect(
        &daemon.docker,
        ENGINE_ID,
        DOCKER_ROOT,
        &recorded_release()?,
        Instant::now() + Duration::from_secs(3),
    )
    .await;
    let requests = daemon.finish().await?;
    Ok((result, requests))
}

async fn serve(
    listener: UnixListener,
    replies: Vec<Reply>,
    mut completion: oneshot::Receiver<()>,
) -> Result<Vec<String>, HostError> {
    let mut requests = Vec::new();
    for reply in replies {
        let (stream, _) = tokio::select! {
            signal = &mut completion => {
                signal.map_err(|_| HostError::Docker)?;
                return Ok(requests);
            }
            accepted = listener.accept() => accepted.map_err(|_| HostError::Docker)?,
        };
        let (request, mut stream) = read_request_line(stream).await?;
        requests.push(request);
        send_response(&mut stream, reply.status, &reply.body).await?;
    }
    tokio::select! {
        signal = &mut completion => signal.map_err(|_| HostError::Docker)?,
        accepted = listener.accept() => {
            let (stream, _) = accepted.map_err(|_| HostError::Docker)?;
            let (request, mut stream) = read_request_line(stream).await?;
            requests.push(request);
            send_response(&mut stream, 500, "{\"message\":\"unexpected request\"}").await?;
            return Err(HostError::Identity);
        }
    }
    match timeout(Duration::from_millis(50), listener.accept()).await {
        Ok(Ok((stream, _))) => {
            let (request, mut stream) = read_request_line(stream).await?;
            requests.push(request);
            send_response(&mut stream, 500, "{\"message\":\"unexpected request\"}").await?;
            Err(HostError::Identity)
        }
        Ok(Err(_)) => Err(HostError::Docker),
        Err(_) => Ok(requests),
    }
}

async fn read_request_line(mut stream: UnixStream) -> Result<(String, UnixStream), HostError> {
    let mut reader = BufReader::new(&mut stream);
    let mut request = String::new();
    let read = reader
        .read_line(&mut request)
        .await
        .map_err(|_| HostError::Docker)?;
    if read == 0 || request.len() > 4096 {
        return Err(HostError::Frame);
    }
    drop(reader);
    Ok((request.trim_end().to_owned(), stream))
}

async fn send_response(stream: &mut UnixStream, status: u16, body: &str) -> Result<(), HostError> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|_| HostError::Docker)
}

fn recorded_release() -> Result<VerifiedRelease, HostError> {
    let config: ContainerConfig = serde_json::from_value(json!({
        "User": IMAGE_USER,
        "Env": [DEFAULT_ENV],
        "Entrypoint": [IMAGE_ENTRYPOINT],
        "WorkingDir": IMAGE_WORKDIR,
        "Labels": {SOURCE_LABEL: "a".repeat(40)},
    }))
    .map_err(|_| HostError::Identity)?;
    let manifest = ProbeManifest {
        source_commit: "a".repeat(40),
        workflow_authority_sha: "a".repeat(40),
        platform: PLATFORM.to_owned(),
        archive_format: ARCHIVE_FORMAT.to_owned(),
        oci_index_sha256: "b".repeat(64),
        image_manifest_digest: CONTAINERD_MANIFEST.to_owned(),
        config_digest: CLASSIC_CONFIG.to_owned(),
        archive_sha256: "c".repeat(64),
    };
    Ok(VerifiedRelease {
        source_sha: manifest.source_commit.clone(),
        authority_sha: manifest.workflow_authority_sha.clone(),
        archive_sha: manifest.archive_sha256.clone(),
        archive: Vec::new(),
        manifest,
        archive_identity: ArchiveIdentity {
            image_manifest_digest: CONTAINERD_MANIFEST.to_owned(),
            image_manifest_media_type: OCI_MANIFEST_MEDIA_TYPE.to_owned(),
            image_manifest_size: 0,
            config_digest: CLASSIC_CONFIG.to_owned(),
            config,
        },
    })
}

fn containerd_image() -> Result<String, HostError> {
    let config = recorded_config()?;
    image_json(
        CONTAINERD_MANIFEST,
        Some(json!({
            "mediaType": OCI_MANIFEST_MEDIA_TYPE,
            "digest": CONTAINERD_MANIFEST,
            "platform": {"os": "linux", "architecture": "amd64"},
        })),
        config,
    )
}

fn classic_image() -> Result<String, HostError> {
    image_json(CLASSIC_CONFIG, None, recorded_config()?)
}

fn index_image() -> Result<String, HostError> {
    image_json(
        &format!("sha256:{}", "d".repeat(64)),
        Some(json!({
            "mediaType": OCI_MANIFEST_MEDIA_TYPE,
            "digest": format!("sha256:{}", "d".repeat(64)),
        })),
        recorded_config()?,
    )
}

fn image_json(
    id: &str,
    descriptor: Option<Value>,
    config: ContainerConfig,
) -> Result<String, HostError> {
    let config = serde_json::to_value(config).map_err(|_| HostError::Identity)?;
    let mut image = json!({"Id": id, "Os": "linux", "Architecture": "amd64", "Config": config});
    if let Some(descriptor) = descriptor {
        image["Descriptor"] = descriptor;
    }
    serde_json::to_string(&image).map_err(|_| HostError::Identity)
}

fn recorded_config() -> Result<ContainerConfig, HostError> {
    let release = recorded_release()?;
    Ok(release.archive_identity.config)
}

fn info_response(engine: &str, root: &str) -> String {
    json!({"ID": engine, "DockerRootDir": root}).to_string()
}

fn assert_inspect_query(request: &str) {
    assert!(request.starts_with("GET /v1.53/images/"), "{request}");
    assert!(request.contains(IMAGE_TAG), "{request}");
    assert!(
        request.ends_with(&format!("?{PLATFORM_QUERY} HTTP/1.1")),
        "{request}"
    );
}

fn assert_no_import(requests: &[String]) {
    assert!(
        requests
            .iter()
            .all(|request| !request.contains("/images/load"))
    );
}

fn socket_path() -> PathBuf {
    std::env::temp_dir().join(format!("velnor-platform-{}.sock", uuid::Uuid::new_v4()))
}

fn remove_socket(path: &PathBuf) -> Result<(), HostError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(HostError::Path),
    }
}

#[cfg(test)]
#[path = "docker_flow_negative_tests.rs"]
mod negative_tests;

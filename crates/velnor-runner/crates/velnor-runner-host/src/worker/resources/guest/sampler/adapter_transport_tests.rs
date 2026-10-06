use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

use super::super::{GuestDockerClient, GuestSampleFailure};
use super::{BollardGuestDocker, MAX_PROBE_ARCHIVE_BYTES};

const PROBE_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);

struct DockerSocketStub {
    docker: Docker,
    path: PathBuf,
    task: Option<JoinHandle<Result<(), io::Error>>>,
}

impl DockerSocketStub {
    fn open(status: u16, body: Vec<u8>) -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-guest-resource-{}-{number}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path)?;
        let socket = path.to_string_lossy();
        let docker =
            match Docker::connect_with_unix(socket.as_ref(), 5, bollard::API_DEFAULT_VERSION) {
                Ok(docker) => docker,
                Err(error) => {
                    let _removed = std::fs::remove_file(&path).err().map(|error| error.kind());
                    return Err(Box::new(error));
                }
            };
        let task = tokio::spawn(async move { respond(listener, status, body).await });
        Ok(Self {
            docker,
            path,
            task: Some(task),
        })
    }

    async fn finish(mut self) -> Result<(), Box<dyn Error>> {
        let task = self
            .task
            .take()
            .ok_or_else(|| io::Error::other("Docker socket stub stopped"))?;
        tokio::time::timeout(RESPONSE_TIMEOUT, task).await???;
        std::fs::remove_file(&self.path)?;
        Ok(())
    }
}

impl Drop for DockerSocketStub {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let _removed = std::fs::remove_file(&self.path)
            .err()
            .map(|error| error.kind());
    }
}

#[tokio::test]
async fn oversized_archive_over_unix_transport_is_rejected() -> Result<(), Box<dyn Error>> {
    let result = read_response(200, vec![b'x'; MAX_PROBE_ARCHIVE_BYTES + 1]).await?;
    assert_eq!(result, Err(GuestSampleFailure::OutputLimit));
    Ok(())
}

#[tokio::test]
async fn malformed_archive_over_unix_transport_is_unknown() -> Result<(), Box<dyn Error>> {
    let result = read_response(200, b"not a tar archive".to_vec()).await?;
    assert_eq!(result, Err(GuestSampleFailure::ProbeOutput));
    Ok(())
}

#[tokio::test]
async fn only_confirmed_unix_transport_404_means_output_not_ready() -> Result<(), Box<dyn Error>> {
    let result = read_response(404, b"not found".to_vec()).await?;
    assert_eq!(result, Ok(None));
    Ok(())
}

async fn read_response(
    status: u16,
    body: Vec<u8>,
) -> Result<Result<Option<Vec<u8>>, GuestSampleFailure>, Box<dyn Error>> {
    let stub = DockerSocketStub::open(status, body)?;
    let docker = BollardGuestDocker::new(stub.docker.clone());
    let result = docker.read_output(PROBE_ID, 512).await;
    stub.finish().await?;
    Ok(result)
}

async fn respond(listener: UnixListener, status: u16, body: Vec<u8>) -> Result<(), io::Error> {
    let (mut stream, _) = tokio::time::timeout(RESPONSE_TIMEOUT, listener.accept())
        .await
        .map_err(io::Error::other)??;
    read_request(&mut stream).await?;
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    };
    let headers = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/x-tar\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await
}

async fn read_request(stream: &mut UnixStream) -> Result<(), io::Error> {
    const MAX_REQUEST_BYTES: usize = 4096;
    let mut request = Vec::with_capacity(256);
    let mut chunk = [0_u8; 512];
    loop {
        let count = tokio::time::timeout(RESPONSE_TIMEOUT, stream.read(&mut chunk))
            .await
            .map_err(io::Error::other)??;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Docker request ended before headers",
            ));
        }
        request.extend_from_slice(&chunk[..count]);
        if request.len() > MAX_REQUEST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Docker request headers exceeded the test limit",
            ));
        }
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let target = format!("/containers/{PROBE_ID}/archive?");
    if !request
        .windows(target.len())
        .any(|window| window == target.as_bytes())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected Docker archive request",
        ));
    }
    Ok(())
}

//! Unix-socket Docker stub for inspect and capacity tests.
//!
//! Every item is `cfg(test)`: production builds see an empty module. The
//! canonical suite form bans `cfg(test)` module declarations, so the stub
//! shared by the inspect, launch, turn, and volumes suites lives here
//! instead of a test-only module.

#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use bollard::Docker;
#[cfg(test)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(test)]
use tokio::net::{UnixListener, UnixStream};

#[cfg(test)]
const TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(test)]
pub(in crate::launch) struct DockerResponse {
    status: Option<u16>,
    body: String,
    hang: bool,
}

#[cfg(test)]
pub(in crate::launch) struct DockerStub {
    pub(in crate::launch) docker: Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

#[cfg(test)]
impl DockerStub {
    pub(in crate::launch) fn open(responses: Vec<DockerResponse>) -> Result<Self, String> {
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

    pub(in crate::launch) async fn finish(mut self) -> Result<(), String> {
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

#[cfg(test)]
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

#[cfg(test)]
pub(in crate::launch) fn http(status: u16, body: &str) -> DockerResponse {
    DockerResponse {
        status: Some(status),
        body: body.to_owned(),
        hang: false,
    }
}

#[cfg(test)]
pub(super) fn closed() -> DockerResponse {
    DockerResponse {
        status: None,
        body: String::new(),
        hang: false,
    }
}

#[cfg(test)]
pub(super) fn hanging() -> DockerResponse {
    DockerResponse {
        status: None,
        body: String::new(),
        hang: true,
    }
}

#[cfg(test)]
async fn send_response(listener: &UnixListener, response: &DockerResponse) -> Result<(), String> {
    let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
    read_request(&mut stream).await?;
    if response.hang {
        std::future::pending::<()>().await;
    }
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

#[cfg(test)]
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

#[cfg(test)]
const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

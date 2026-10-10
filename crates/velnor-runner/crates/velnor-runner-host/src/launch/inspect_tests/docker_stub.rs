use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;

use super::TIMEOUT;

pub(in crate::launch) struct DockerResponse {
    status: Option<u16>,
    body: String,
    hang: bool,
}

pub(in crate::launch) struct DockerStub {
    pub(in crate::launch) docker: Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<Vec<String>, String>>>,
    stop: Option<oneshot::Sender<()>>,
    expected: usize,
}

impl DockerStub {
    pub(in crate::launch) fn open(responses: Vec<DockerResponse>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-inspect-{}-{number}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        let expected = responses.len();
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut requests = Vec::with_capacity(expected);
            for response in responses {
                let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
                requests.push(read_request(&mut stream).await?);
                send_response(&mut stream, &response).await?;
            }
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (mut stream, _) = accepted.map_err(|error| error.to_string())?;
                        requests.push(read_request(&mut stream).await?);
                        send_response(
                            &mut stream,
                            &DockerResponse {
                                status: Some(500),
                                body: r#"{"message":"unexpected Docker request"}"#.to_owned(),
                                hang: false,
                            },
                        )
                        .await?;
                    }
                    _ = &mut stopped => break,
                }
            }
            Ok(requests)
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
            stop: Some(stop),
            expected,
        })
    }

    pub(in crate::launch) async fn finish(mut self) -> Result<Vec<String>, String> {
        let stop = self
            .stop
            .take()
            .ok_or_else(|| "Docker stub already stopped".to_owned())?;
        stop.send(())
            .map_err(|()| "Docker stub already stopped".to_owned())?;
        let mut task = self
            .task
            .take()
            .ok_or_else(|| "Docker stub already stopped".to_owned())?;
        let served = if let Ok(result) = tokio::time::timeout(TIMEOUT, &mut task).await {
            result.map_err(|error| error.to_string())?
        } else {
            task.abort();
            return Err("Docker stub timed out waiting to stop".to_owned());
        };
        let removed = std::fs::remove_file(&self.path).map_err(|error| error.to_string());
        let requests = served?;
        removed?;
        if requests.len() != self.expected {
            return Err(format!(
                "expected {} Docker requests, got {}",
                self.expected,
                requests.len()
            ));
        }
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

pub(in crate::launch) fn http(status: u16, body: &str) -> DockerResponse {
    DockerResponse {
        status: Some(status),
        body: body.to_owned(),
        hang: false,
    }
}

pub(super) fn closed() -> DockerResponse {
    DockerResponse {
        status: None,
        body: String::new(),
        hang: false,
    }
}

pub(in crate::launch) fn hanging() -> DockerResponse {
    DockerResponse {
        status: None,
        body: String::new(),
        hang: true,
    }
}

async fn send_response(stream: &mut UnixStream, response: &DockerResponse) -> Result<(), String> {
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

async fn read_request(stream: &mut UnixStream) -> Result<String, String> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 256];
    loop {
        let read = stream
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("Docker client closed before sending a request".to_owned());
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(2).any(|bytes| bytes == b"\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&request).map_err(|error| error.to_string())?;
    request
        .lines()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "Docker client sent an empty request".to_owned())
}

const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

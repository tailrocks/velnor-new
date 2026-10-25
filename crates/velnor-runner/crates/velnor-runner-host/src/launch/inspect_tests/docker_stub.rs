use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;

use super::TIMEOUT;

mod request;
#[path = "docker_stub/threaded.rs"]
pub(crate) mod threaded;

pub(crate) struct DockerResponse {
    status: Option<u16>,
    body: Vec<u8>,
    delay: Duration,
}

pub(crate) struct DockerStub {
    pub(crate) docker: Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<Vec<String>, String>>>,
    stop: Option<oneshot::Sender<()>>,
    expected: usize,
    observed: Arc<Mutex<Vec<request::RequestTrace>>>,
}

impl DockerStub {
    pub(crate) fn open(responses: Vec<DockerResponse>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-inspect-{}-{number}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        let expected = responses.len();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let task_observed = Arc::clone(&observed);
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut requests = Vec::with_capacity(expected);
            let mut responses = responses.into_iter();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (mut stream, _) = accepted.map_err(|error| error.to_string())?;
                        let line = match read_request(&mut stream).await {
                            Ok(line) => line,
                            Err(failure) => {
                                task_observed.lock().map_err(|_| "The test could not lock the Docker request trace.".to_owned())?.push(request::RequestTrace {
                                    path: failure.path,
                                    response_index: None,
                                    response_status: None,
                                    failure: Some(failure.message.clone()),
                                });
                                return Err(failure.message);
                            }
                        };
                        requests.push(line.clone());
                        let response = responses.next().unwrap_or(DockerResponse {
                                status: Some(500),
                                body: br#"{"message":"unexpected Docker request"}"#.to_vec(),
                                delay: Duration::ZERO,
                            });
                        let response_index = requests.len() - 1;
                        task_observed.lock().map_err(|_| "The test could not lock the Docker request trace.".to_owned())?.push(request::RequestTrace {
                            path: Some(request::request_path(&line)?),
                            response_index: Some(response_index),
                            response_status: response.status,
                            failure: None,
                        });
                        send_response(&mut stream, &response).await?;
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
            observed,
        })
    }

    pub(crate) async fn finish(mut self) -> Result<Vec<String>, String> {
        self.collect(true).await
    }

    pub(crate) async fn finish_observed(mut self) -> Result<Vec<String>, String> {
        self.collect(false).await
    }

    pub(crate) fn observed_request_summary(&self) -> Result<String, String> {
        self.observed
            .lock()
            .map(|requests| summarize_requests(&requests))
            .map_err(|_| "The test could not lock the Docker request trace.".to_owned())
    }

    async fn collect(&mut self, require_expected: bool) -> Result<Vec<String>, String> {
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
        if require_expected && requests.len() != self.expected {
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

pub(crate) fn http(status: u16, body: &str) -> DockerResponse {
    DockerResponse {
        status: Some(status),
        body: body.as_bytes().to_vec(),
        delay: Duration::ZERO,
    }
}

pub(crate) fn raw_http(status: u16, body: Vec<u8>) -> DockerResponse {
    DockerResponse {
        status: Some(status),
        body,
        delay: Duration::ZERO,
    }
}

pub(in crate::launch) fn delayed_http(status: u16, body: &str, delay: Duration) -> DockerResponse {
    DockerResponse {
        status: Some(status),
        body: body.as_bytes().to_vec(),
        delay,
    }
}

pub(in crate::launch) fn closed() -> DockerResponse {
    DockerResponse {
        status: None,
        body: Vec::new(),
        delay: Duration::ZERO,
    }
}

async fn send_response(stream: &mut UnixStream, response: &DockerResponse) -> Result<(), String> {
    tokio::time::sleep(response.delay).await;
    let Some(status) = response.status else {
        return Ok(());
    };
    let message = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        response.body.len()
    );
    if let Err(error) = stream.write_all(message.as_bytes()).await {
        return disconnected_peer(&error)
            .then_some(())
            .ok_or_else(|| error.to_string());
    }
    if let Err(error) = stream.write_all(&response.body).await {
        return disconnected_peer(&error)
            .then_some(())
            .ok_or_else(|| error.to_string());
    }
    Ok(())
}

fn disconnected_peer(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::NotConnected
    )
}

fn summarize_requests(requests: &[request::RequestTrace]) -> String {
    requests
        .iter()
        .map(|request| {
            format!(
                "{} [response {} status {:?}]",
                request.path.as_deref().unwrap_or("<unparsed>"),
                request
                    .response_index
                    .map_or_else(|| "none".to_owned(), |index| index.to_string()),
                request.response_status
            ) + &request
                .failure
                .as_ref()
                .map_or_else(String::new, |failure| {
                    format!(" [request failure: {failure}]")
                })
        })
        .collect::<Vec<_>>()
        .join("; ")
}

async fn read_request(stream: &mut UnixStream) -> Result<String, request::RequestFailure> {
    let mut framing = request::RequestFraming::default();
    let mut buffer = [0_u8; 256];
    loop {
        let read = match stream.read(&mut buffer).await {
            Ok(read) => read,
            Err(error) => return Err(framing.failure(error.to_string())),
        };
        if read == 0 {
            return framing
                .finish()
                .map(str::to_owned)
                .map_err(|error| framing.failure(error));
        }
        let complete = framing
            .push(&buffer[..read])
            .map_err(|error| framing.failure(error))?;
        if complete {
            return framing
                .finish()
                .map(str::to_owned)
                .map_err(|error| framing.failure(error));
        }
    }
}

const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

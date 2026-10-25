use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub(super) struct RecordedRequest {
    pub(super) line: String,
    pub(super) body: Vec<u8>,
}

pub(super) struct DockerBodyStub {
    pub(super) docker: Docker,
    path: PathBuf,
    task: Option<JoinHandle<Result<Vec<RecordedRequest>, String>>>,
    stop: Option<oneshot::Sender<()>>,
    expected: usize,
}

impl DockerBodyStub {
    pub(super) fn open(responses: Vec<(u16, String)>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-volume-body-{}-{id}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        let expected = responses.len();
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut requests = Vec::with_capacity(expected);
            let mut responses = responses.into_iter();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (mut stream, _) = accepted.map_err(|error| error.to_string())?;
                        requests.push(read_request(&mut stream).await?);
                        let (status, body) = responses.next().unwrap_or_else(|| {
                            (500, r#"{"message":"unexpected Docker request"}"#.to_owned())
                        });
                        write_response(&mut stream, status, body.as_bytes()).await?;
                    }
                    _ = &mut stopped => break,
                }
            }
            Ok(requests)
        });
        let socket = path
            .to_str()
            .ok_or_else(|| "Docker socket path is not UTF-8".to_owned())?;
        let docker = Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            docker,
            path,
            task: Some(task),
            stop: Some(stop),
            expected,
        })
    }

    pub(super) async fn finish(mut self) -> Result<Vec<RecordedRequest>, String> {
        let stop = self
            .stop
            .take()
            .ok_or_else(|| "Docker body stub already stopped".to_owned())?;
        stop.send(())
            .map_err(|()| "Docker body stub already stopped".to_owned())?;
        let mut task = self
            .task
            .take()
            .ok_or_else(|| "Docker body stub already stopped".to_owned())?;
        let served = tokio::time::timeout(Duration::from_secs(2), &mut task)
            .await
            .map_err(|_| "Docker body stub timed out".to_owned())?
            .map_err(|error| error.to_string())??;
        std::fs::remove_file(&self.path).map_err(|error| error.to_string())?;
        if served.len() != self.expected {
            return Err(format!(
                "expected {} Docker requests, got {}",
                self.expected,
                served.len()
            ));
        }
        Ok(served)
    }
}

impl Drop for DockerBodyStub {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let _removed = std::fs::remove_file(&self.path)
            .err()
            .map(|error| error.kind());
    }
}

async fn read_request(stream: &mut UnixStream) -> Result<RecordedRequest, String> {
    let mut data = Vec::new();
    let header_end = loop {
        if let Some(end) = find_bytes(&data, b"\r\n\r\n") {
            break end;
        }
        read_more(stream, &mut data).await?;
    };
    let header =
        String::from_utf8(data[..header_end].to_vec()).map_err(|error| error.to_string())?;
    let length = content_length(&header)?;
    while data.len() < header_end + 4 + length {
        read_more(stream, &mut data).await?;
    }
    let line = header
        .lines()
        .next()
        .ok_or_else(|| "Docker sent an empty request".to_owned())?
        .to_owned();
    let body = data[header_end + 4..header_end + 4 + length].to_vec();
    Ok(RecordedRequest { line, body })
}

async fn read_more(stream: &mut UnixStream, data: &mut Vec<u8>) -> Result<(), String> {
    let mut buffer = [0_u8; 1024];
    let size = stream
        .read(&mut buffer)
        .await
        .map_err(|error| error.to_string())?;
    if size == 0 {
        return Err("Docker closed a request before its body was complete".to_owned());
    }
    data.extend_from_slice(&buffer[..size]);
    Ok(())
}

fn content_length(header: &str) -> Result<usize, String> {
    header
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| {
            value
                .trim()
                .parse::<usize>()
                .map_err(|error| error.to_string())
        })
        .transpose()
        .map(|length| length.unwrap_or(0))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn write_response(stream: &mut UnixStream, status: u16, body: &[u8]) -> Result<(), String> {
    let reason = if status == 200 { "OK" } else { "Response" };
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

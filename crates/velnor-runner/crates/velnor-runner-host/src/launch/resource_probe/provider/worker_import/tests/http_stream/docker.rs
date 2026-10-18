use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bollard::Docker;
use tokio::net::UnixListener;
use tokio::sync::oneshot;

use crate::journal::{ImageImportRequest, ImageImportState, Journal};

const MAX_CAPTURE_BYTES: usize = 32 * 1024 * 1024;

mod protocol;
mod serve;

pub(super) enum ImportReply {
    Complete { status: u16, body: Vec<u8> },
    Early { status: u16, body: Vec<u8> },
    Truncated,
    DisconnectAfterBody,
    Hang,
}

pub(super) enum Reply {
    Http {
        status: u16,
        body: Vec<u8>,
    },
    Import {
        mode: ImportReply,
        state_probe: Box<(Journal, ImageImportRequest)>,
    },
}

impl Reply {
    pub(super) fn http(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self::Http {
            status,
            body: body.into(),
        }
    }

    pub(super) fn import(
        mode: ImportReply,
        journal: &Journal,
        request: &ImageImportRequest,
    ) -> Self {
        Self::Import {
            mode,
            state_probe: Box::new((journal.clone(), request.clone())),
        }
    }
}

#[derive(Debug)]
pub(super) struct CapturedRequest {
    pub(super) line: String,
    pub(super) body: Vec<u8>,
    pub(super) body_complete: bool,
    pub(super) state_at_import: Option<ImageImportState>,
}

pub(super) struct ImportDockerStub {
    pub(super) docker: Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<Vec<CapturedRequest>, String>>>,
    stop: Option<oneshot::Sender<()>>,
    expected: usize,
}

impl ImportDockerStub {
    pub(super) fn open(replies: Vec<Reply>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-worker-import-{}-{number}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        let expected = replies.len();
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut captured = Vec::with_capacity(expected);
            let mut replies = VecDeque::from(replies);
            loop {
                let accepted = tokio::select! {
                    accepted = listener.accept() => accepted.map_err(|error| error.to_string())?,
                    _ = &mut stopped => break,
                };
                let (mut stream, _) = accepted;
                let reply = replies.pop_front().unwrap_or_else(|| {
                    Reply::http(500, br#"{"message":"unexpected Docker request"}"#.to_vec())
                });
                let (request, stop_server) =
                    serve::serve_request(&mut stream, reply, &mut stopped).await?;
                captured.push(request);
                if stop_server {
                    break;
                }
            }
            Ok(captured)
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

    pub(super) async fn finish(self) -> Result<Vec<CapturedRequest>, String> {
        let (captured, expected) = self.stop_and_join().await?;
        if captured.len() != expected {
            return Err(format!(
                "expected {expected} Docker requests, received {}",
                captured.len()
            ));
        }
        Ok(captured)
    }

    pub(super) async fn finish_observed(self) -> Result<Vec<CapturedRequest>, String> {
        self.stop_and_join().await.map(|(captured, _)| captured)
    }

    async fn stop_and_join(mut self) -> Result<(Vec<CapturedRequest>, usize), String> {
        let stop = self
            .stop
            .take()
            .ok_or_else(|| "Docker import stub already stopped".to_owned())?;
        stop.send(())
            .map_err(|()| "Docker import stub already stopped".to_owned())?;
        let mut task = self
            .task
            .take()
            .ok_or_else(|| "Docker import stub already stopped".to_owned())?;
        let captured = tokio::time::timeout(Duration::from_secs(3), &mut task)
            .await
            .map_err(|_| "Docker import stub timed out while stopping".to_owned())?
            .map_err(|error| error.to_string())??;
        std::fs::remove_file(&self.path).map_err(|error| error.to_string())?;
        Ok((captured, self.expected))
    }
}

impl Drop for ImportDockerStub {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let _removed = std::fs::remove_file(&self.path)
            .err()
            .map(|error| error.kind());
    }
}

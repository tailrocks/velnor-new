use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Context, Waker};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;

use crate::worker::{
    VerifiedWorkerVolume, WorkerVolumeRemoval, WorkerVolumeRole, WorkerVolumeVerification,
    remove_verified_worker_volume, verify_worker_volume,
};
use crate::{HostError, start_pair};

const WORKER: &str = "wtransport";
const WORK_VOLUME: &str = "wtransport-work";

#[tokio::test]
async fn empty_jit_does_not_create() -> Result<(), String> {
    let idle = ScriptedDocker::open(Vec::new())?;
    for volume in ["a/b", "worker_a"] {
        assert_eq!(
            start_pair(
                &idle.docker,
                volume,
                crate::worker::test_resource_budget().map_err(|error| error.to_string())?,
                b"",
            )
            .await,
            Err(HostError::EmptyJit)
        );
    }
    assert_eq!(idle.finish().await?, Vec::new());
    Ok(())
}

#[tokio::test]
async fn exact_volume_verification_rejects_wrong_owner_role_or_name() -> Result<(), String> {
    for (name, owner, role) in [
        (WORK_VOLUME, "someone_else", "work"),
        (WORK_VOLUME, WORKER, "socket"),
        ("wrong-name", WORKER, "work"),
    ] {
        let stub = ScriptedDocker::open(vec![(200, volume_json(name, owner, role))])?;
        let result = verify_worker_volume(&stub.docker, WORKER, WorkerVolumeRole::Work).await;
        let requests = stub.finish().await?;

        assert_eq!(result, Ok(WorkerVolumeVerification::OwnershipMismatch));
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET "));
    }
    Ok(())
}

#[tokio::test]
async fn exact_volume_removal_treats_only_not_found_as_absent() -> Result<(), String> {
    let absent = ScriptedDocker::open(vec![(404, r#"{"message":"missing"}"#.to_owned())])?;
    let result = verify_worker_volume(&absent.docker, WORKER, WorkerVolumeRole::Socket).await;
    assert_eq!(result, Ok(WorkerVolumeVerification::Absent));
    assert_eq!(absent.finish().await?.len(), 1);

    let failed = ScriptedDocker::open(vec![(500, r#"{"message":"busy"}"#.to_owned())])?;
    let result = verify_worker_volume(&failed.docker, WORKER, WorkerVolumeRole::Socket).await;
    assert_eq!(result, Err(HostError::Docker));
    assert_eq!(failed.finish().await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn exact_volume_remove_errors_and_confirms_single_target() -> Result<(), String> {
    let stub = ScriptedDocker::open(vec![
        (200, volume_json(WORK_VOLUME, WORKER, "work")),
        (500, r#"{"message":"busy"}"#.to_owned()),
        (200, volume_json(WORK_VOLUME, WORKER, "work")),
        (204, String::new()),
        (404, r#"{"message":"missing"}"#.to_owned()),
        (200, volume_json(WORK_VOLUME, WORKER, "work")),
        (204, String::new()),
        (503, r#"{"message":"unavailable"}"#.to_owned()),
    ])?;
    let failed = verified_work_volume(&stub.docker).await?;
    assert_eq!(
        remove_verified_worker_volume(&stub.docker, &failed).await,
        Err(HostError::Docker)
    );
    let verified = verified_work_volume(&stub.docker).await?;
    assert_eq!(
        remove_verified_worker_volume(&stub.docker, &verified).await,
        Ok(WorkerVolumeRemoval::Removed)
    );
    let uncertain = verified_work_volume(&stub.docker).await?;
    assert_eq!(
        remove_verified_worker_volume(&stub.docker, &uncertain).await,
        Err(HostError::Docker)
    );
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 8);
    assert_eq!(
        requests
            .iter()
            .map(|request| request.starts_with("DELETE "))
            .collect::<Vec<_>>(),
        [false, true, false, true, false, false, true, false]
    );
    assert!(requests.iter().all(|request| request.contains(WORK_VOLUME)));
    Ok(())
}

#[tokio::test]
async fn cancelling_finish_aborts_stub_and_cleans_socket() -> Result<(), String> {
    let stub = ScriptedDocker::open(vec![(200, String::new())])?;
    let path = stub.path.clone();
    let started = Arc::clone(&stub.server_started);
    let stopped = Arc::clone(&stub.server_stopped);
    wait_for_flag(started).await?;

    let mut finish = Box::pin(stub.finish());
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(finish.as_mut().poll(&mut context).is_pending());
    drop(finish);

    assert!(!path.exists());
    wait_for_flag(stopped).await
}

#[tokio::test]
async fn stub_catches_late_request_before_finish_signal() -> Result<(), String> {
    let stub = ScriptedDocker::open(Vec::new())?;
    let path = stub.path.clone();
    let started = Arc::clone(&stub.server_started);
    let stopped = Arc::clone(&stub.server_stopped);
    wait_for_flag(started).await?;
    tokio::time::sleep(Duration::from_millis(75)).await;
    assert!(!stopped.load(Ordering::SeqCst));

    let mut connection = UnixStream::connect(&path)
        .await
        .map_err(|error| error.to_string())?;
    connection
        .write_all(b"GET /late HTTP/1.1\r\nHost: docker\r\n\r\n")
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    wait_for_flag(stopped).await?;

    let result = stub.finish().await;
    assert!(matches!(
        result,
        Err(error) if error.contains("unexpected request") && error.contains("GET /late HTTP/1.1")
    ));
    assert!(!path.exists());
    Ok(())
}

async fn verified_work_volume(docker: &bollard::Docker) -> Result<VerifiedWorkerVolume, String> {
    match verify_worker_volume(docker, WORKER, WorkerVolumeRole::Work)
        .await
        .map_err(|error| error.to_string())?
    {
        WorkerVolumeVerification::Verified(volume) => Ok(volume),
        _ => Err("expected the matching volume to be verified".to_owned()),
    }
}

struct ScriptedDocker {
    docker: bollard::Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<Vec<String>, String>>>,
    completion: Option<oneshot::Sender<()>>,
    server_started: Arc<AtomicBool>,
    server_stopped: Arc<AtomicBool>,
    finished: bool,
}

impl ScriptedDocker {
    fn open(responses: Vec<(u16, String)>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-exact-volume-{}-{n}.sock",
            std::process::id()
        ));
        let socket = path
            .to_str()
            .ok_or_else(|| "socket path is not UTF-8".to_owned())?;
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        let docker =
            match bollard::Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION) {
                Ok(docker) => docker,
                Err(error) => {
                    if let Err(cleanup_error) = std::fs::remove_file(&path) {
                        return Err(format!("{error}; socket cleanup failed: {cleanup_error}"));
                    }
                    return Err(error.to_string());
                }
            };
        let server_started = Arc::new(AtomicBool::new(false));
        let server_stopped = Arc::new(AtomicBool::new(false));
        let task_started = Arc::clone(&server_started);
        let task_stopped = Arc::clone(&server_stopped);
        let (completion, wait_for_completion) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _completion = TaskCompletion(task_stopped);
            task_started.store(true, Ordering::SeqCst);
            serve_volume_stub(listener, responses, wait_for_completion).await
        });
        Ok(Self {
            docker,
            path,
            task: Some(task),
            completion: Some(completion),
            server_started,
            server_stopped,
            finished: false,
        })
    }

    async fn finish(mut self) -> Result<Vec<String>, String> {
        let signal_error = self
            .completion
            .take()
            .ok_or_else(|| "stub completion already signaled".to_owned())?
            .send(())
            .err()
            .map(|()| "stub stopped before completion signal".to_owned());
        let result = {
            let task = self
                .task
                .as_mut()
                .ok_or_else(|| "stub already stopped".to_owned())?;
            match tokio::time::timeout(Duration::from_secs(2), task).await {
                Ok(result) => result.map_err(|error| error.to_string())??,
                Err(_) => return Err("Docker stub timed out".to_owned()),
            }
        };
        if let Some(error) = signal_error {
            return Err(error);
        }
        self.task = None;
        remove_socket_after_finish(&self.path)?;
        self.finished = true;
        Ok(result)
    }
}

struct TaskCompletion(Arc<AtomicBool>);

impl Drop for TaskCompletion {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl Drop for ScriptedDocker {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if !self.finished
            && let Err(error) = std::fs::remove_file(&self.path)
        {
            eprintln!("Docker stub socket cleanup failed: {error}");
        }
    }
}

async fn serve_volume_stub(
    listener: UnixListener,
    responses: Vec<(u16, String)>,
    mut wait_for_completion: oneshot::Receiver<()>,
) -> Result<Vec<String>, String> {
    let mut requests = Vec::new();
    for (status, body) in responses {
        let (stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let mut stream = BufReader::new(stream);
        let mut request = String::new();
        let _read = stream
            .read_line(&mut request)
            .await
            .map_err(|error| error.to_string())?;
        requests.push(request.trim_end().to_owned());
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .get_mut()
            .write_all(response.as_bytes())
            .await
            .map_err(|error| error.to_string())?;
    }
    tokio::select! {
        signal = &mut wait_for_completion => {
            signal.map_err(|error| error.to_string())?;
        }
        accepted = listener.accept() => {
            let (stream, _) = accepted.map_err(|error| error.to_string())?;
            let request = read_request_line(stream).await?;
            return Err(format!("unexpected request before finish: {request}"));
        }
    }
    match tokio::time::timeout(Duration::from_millis(50), listener.accept()).await {
        Ok(Ok((stream, _))) => {
            let request = read_request_line(stream).await?;
            Err(format!("unexpected request after finish: {request}"))
        }
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Ok(requests),
    }
}

async fn read_request_line(stream: UnixStream) -> Result<String, String> {
    let mut stream = BufReader::new(stream);
    let mut request = String::new();
    stream
        .read_line(&mut request)
        .await
        .map_err(|error| error.to_string())?;
    Ok(request.trim_end().to_owned())
}

fn volume_json(name: &str, worker: &str, role: &str) -> String {
    serde_json::json!({
        "Name": name,
        "Driver": "local",
        "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"),
        "Labels": {"velnor.worker": worker, "velnor.role": role},
        "Options": {},
        "Scope": "local"
    })
    .to_string()
}

fn remove_socket_after_finish(path: &PathBuf) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

async fn wait_for_flag(flag: Arc<AtomicBool>) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(1), async {
        while !flag.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|error| error.to_string())
}

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

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
            start_pair(&idle.docker, volume, b"").await,
            Err(HostError::EmptyJit)
        );
    }
    assert!(idle.finish().await?.is_empty());
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
        let task = tokio::spawn(serve_volume_stub(listener, responses));
        Ok(Self {
            docker,
            path,
            task: Some(task),
            finished: false,
        })
    }

    async fn finish(mut self) -> Result<Vec<String>, String> {
        let mut task = self
            .task
            .take()
            .ok_or_else(|| "stub already stopped".to_owned())?;
        let result = match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
            Ok(result) => result.map_err(|error| error.to_string())??,
            Err(_) => {
                task.abort();
                return Err("Docker stub timed out".to_owned());
            }
        };
        remove_socket_after_finish(&self.path)?;
        self.finished = true;
        Ok(result)
    }
}

impl Drop for ScriptedDocker {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if !self.finished {
            if let Err(error) = std::fs::remove_file(&self.path) {
                eprintln!("Docker stub socket cleanup failed: {error}");
            }
        }
    }
}

async fn serve_volume_stub(
    listener: UnixListener,
    responses: Vec<(u16, String)>,
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
    if tokio::time::timeout(Duration::from_millis(50), listener.accept())
        .await
        .is_ok()
    {
        return Err("unexpected request after scripted responses".to_owned());
    }
    Ok(requests)
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

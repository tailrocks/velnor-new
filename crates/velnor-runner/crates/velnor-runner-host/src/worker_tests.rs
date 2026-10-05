//! Create projection. No live Docker daemon.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bollard::models::MountType;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use crate::worker::{
    VerifiedWorkerVolume, WorkerVolumeRemoval, WorkerVolumeRole, WorkerVolumeVerification,
    remove_verified_worker_volume, verify_worker_volume,
};
use crate::{
    BollardCreate, CreateProjection, HostError, bollard_create, connect_unix, dind_create,
    runner_create, runner_plan, start_pair,
};

fn projection(volume: &str) -> Result<CreateProjection, HostError> {
    runner_create(&runner_plan(volume)?)
}

fn bollard(volume: &str) -> Result<BollardCreate, HostError> {
    bollard_create(&projection(volume)?)
}

#[test]
fn runner_create_opens_stdin_and_is_not_privileged() -> Result<(), HostError> {
    let spec = projection("worker_a")?;
    assert_eq!(spec.name, "worker_a-runner");
    assert!(spec.open_stdin);
    assert!(!spec.privileged);
    assert_eq!(spec.platform, "linux/amd64");
    assert_eq!(spec.image, "velnor-runner:ubuntu-26.04-2.337.0");
    assert_eq!(spec.mounts.len(), 2);
    assert_eq!(spec.mounts[0].source, "volume:worker_a");
    assert_eq!(spec.mounts[0].target, "/run");
    assert!(
        spec.mounts
            .iter()
            .all(|mount| mount.target != "/var/lib/docker")
    );
    assert_eq!(spec.env, Vec::<String>::new());
    assert!(spec.network_mode.is_none());
    Ok(())
}

#[test]
fn runner_create_rejects_jit_in_env_or_cmd() -> Result<(), HostError> {
    let mut plan = runner_plan("worker_a")?;
    plan.env
        .push("ACTIONS_RUNNER_INPUT_JITCONFIG=canary".to_owned());
    assert_eq!(runner_create(&plan), Err(HostError::ForbiddenMount));

    let mut plan = runner_plan("worker_a")?;
    plan.cmd.push("canary-jit".to_owned());
    assert_eq!(runner_create(&plan), Err(HostError::ForbiddenMount));
    Ok(())
}

#[test]
fn dind_is_privileged_and_shares_the_runner_volumes() -> Result<(), HostError> {
    for name in ["", "a/b", "a b", ".hidden", "has:colon"] {
        assert_eq!(dind_create(name).err(), runner_plan(name).err(), "{name}");
    }
    let spec = dind_create("worker_a")?;
    assert_eq!(spec.name, "worker_a-dind");
    assert!(spec.labels.contains(&"velnor.worker=worker_a".to_owned()));
    assert!(spec.labels.contains(&"velnor.role=dind".to_owned()));
    assert!(spec.privileged);
    assert!(!spec.open_stdin);
    assert_eq!(spec.env, Vec::<String>::new());
    assert_eq!(spec.cmd, Vec::<String>::new());
    assert_eq!(spec.image, "velnor-dind:29.8.2");
    assert_eq!(spec.platform, "linux/amd64");
    let runner = runner_plan("worker_a")?;
    assert_eq!(&spec.mounts[..runner.mounts.len()], &runner.mounts[..]);
    assert!(
        !runner
            .mounts
            .iter()
            .any(|mount| mount.target == "/var/lib/docker")
    );
    assert_eq!(
        spec.mounts
            .last()
            .map(|mount| (mount.source.as_str(), mount.target.as_str())),
        Some(("volume:worker_a-docker", "/var/lib/docker"))
    );
    assert!(spec.network_mode.is_none());
    Ok(())
}

#[test]
fn runner_joins_only_its_dind_netns() -> Result<(), HostError> {
    let spec = projection("worker_a")?;
    let not_hex = "g".repeat(64);
    for bad in [
        "",
        "host",
        "container:abc",
        "../id",
        "short",
        not_hex.as_str(),
    ] {
        assert_eq!(
            crate::worker::join_dind_net(spec.clone(), bad).err(),
            Some(HostError::ForbiddenMount),
            "{bad}"
        );
    }
    let id = "a".repeat(64);
    let mode = format!("container:{id}");
    let joined = crate::worker::join_dind_net(spec, &id)?;
    assert!(!joined.privileged);
    assert_eq!(joined.network_mode.as_deref(), Some(mode.as_str()));
    let created = bollard_create(&joined)?;
    let host = created
        .config
        .host_config
        .as_ref()
        .ok_or(HostError::Docker)?;
    assert_eq!(host.privileged, Some(false));
    assert_eq!(host.network_mode.as_deref(), Some(mode.as_str()));
    let dind = bollard_create(&dind_create("worker_a")?)?;
    let dind_host = dind.config.host_config.as_ref().ok_or(HostError::Docker)?;
    assert!(dind_host.network_mode.is_none());
    Ok(())
}

#[test]
fn bollard_config_from_a_clean_plan_omits_canary() -> Result<(), HostError> {
    let created = bollard("worker_a")?;
    assert_eq!(created.options.name.as_deref(), Some("worker_a-runner"));
    assert_eq!(created.options.platform, "linux/amd64");
    assert_eq!(created.config.open_stdin, Some(true));
    let host = created
        .config
        .host_config
        .as_ref()
        .ok_or(HostError::Docker)?;
    assert_eq!(host.privileged, Some(false));
    let text = format!("{created:?}");
    assert!(!text.contains("canary-jit"));
    assert!(!text.to_ascii_lowercase().contains("jitconfig"));

    let dind = bollard_create(&dind_create("worker_a")?)?;
    assert_eq!(dind.options.name.as_deref(), Some("worker_a-dind"));
    assert_eq!(dind.options.platform, "linux/amd64");
    assert_eq!(dind.config.open_stdin, Some(false));
    assert!(dind.config.env.is_none());
    assert!(dind.config.cmd.is_none());
    let host = dind.config.host_config.as_ref().ok_or(HostError::Docker)?;
    assert_eq!(host.privileged, Some(true));
    let mounts = host.mounts.as_ref().ok_or(HostError::Docker)?;
    assert_eq!(mounts.len(), 3);
    assert_eq!(mounts[0].target.as_deref(), Some("/run"));
    assert_eq!(mounts[0].source.as_deref(), Some("worker_a"));
    assert_eq!(mounts[0].typ, Some(MountType::VOLUME));
    assert_eq!(mounts[1].target.as_deref(), Some("/home/runner/_work"));
    assert_eq!(mounts[1].source.as_deref(), Some("worker_a-work"));
    assert_eq!(mounts[1].typ, Some(MountType::VOLUME));
    assert_eq!(mounts[2].target.as_deref(), Some("/var/lib/docker"));
    assert_eq!(mounts[2].source.as_deref(), Some("worker_a-docker"));
    assert_eq!(mounts[2].typ, Some(MountType::VOLUME));
    let text = format!("{dind:?}");
    assert!(!text.contains("canary-jit"));
    assert!(!text.contains("/var/run/docker.sock"));
    Ok(())
}

struct IdleDocker {
    path: PathBuf,
    docker: bollard::Docker,
}

impl IdleDocker {
    fn open() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let tick = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Docker)?
            .as_nanos();
        let path = PathBuf::from(format!(
            "/tmp/velnor-w-{}-{n}-{tick}.sock",
            std::process::id()
        ));
        let listener =
            std::os::unix::net::UnixListener::bind(&path).map_err(|_| HostError::Docker)?;
        let text = path.to_str().ok_or(HostError::Path)?;
        let docker = connect_unix(text)?;
        drop(listener);
        Ok(Self { path, docker })
    }
}

impl Drop for IdleDocker {
    fn drop(&mut self) {
        let removed = std::fs::remove_file(&self.path);
        let _kept = removed.err().map(|err| err.kind());
    }
}

#[test]
fn bollard_create_rejects_a_host_bind() -> Result<(), HostError> {
    let mut spec = dind_create("worker_a")?;
    spec.mounts[0].source = "/var/run/docker.sock".to_owned();
    assert_eq!(bollard_create(&spec), Err(HostError::ForbiddenMount));
    Ok(())
}

#[tokio::test]
async fn empty_jit_does_not_create() -> Result<(), HostError> {
    let idle = IdleDocker::open()?;
    assert_eq!(
        start_pair(&idle.docker, "a/b", b"").await,
        Err(HostError::EmptyJit)
    );
    assert_eq!(
        start_pair(&idle.docker, "worker_a", b"").await,
        Err(HostError::EmptyJit)
    );
    Ok(())
}

#[test]
fn malformed_ownership_label_is_rejected() -> Result<(), HostError> {
    let mut spec = projection("worker_a")?;
    spec.labels.push("velnor.volume".to_owned());
    assert_eq!(bollard_create(&spec), Err(HostError::ForbiddenMount));
    Ok(())
}

#[tokio::test]
async fn exact_volume_removal_refuses_wrong_owner_or_role() -> Result<(), String> {
    for (owner, role) in [("someone_else", "work"), (WORKER, "socket")] {
        let name = format!("{WORKER}-work");
        let stub = ScriptedDocker::open(vec![(200, volume_json(&name, owner, role))])?;
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
async fn exact_volume_remove_error_and_success_is_exact() -> Result<(), String> {
    let name = format!("{WORKER}-work");
    let stub = ScriptedDocker::open(vec![
        (200, volume_json(&name, WORKER, "work")),
        (500, r#"{"message":"busy"}"#.to_owned()),
        (200, volume_json(&name, WORKER, "work")),
        (204, String::new()),
        (404, r#"{"message":"missing"}"#.to_owned()),
    ])?;
    let failed = verified_work_volume(&stub.docker).await?;
    assert_eq!(
        remove_verified_worker_volume(&stub.docker, &failed).await,
        Err(HostError::Docker)
    );
    let verified = verified_work_volume(&stub.docker).await?;
    let result = remove_verified_worker_volume(&stub.docker, &verified).await;
    let requests = stub.finish().await?;
    assert_eq!(result, Ok(WorkerVolumeRemoval::Removed));
    assert_eq!(requests.len(), 5);
    let delete_count = requests
        .iter()
        .filter(|request| request.starts_with("DELETE "))
        .count();
    assert_eq!(delete_count, 2);
    assert!(
        requests
            .iter()
            .all(|request| request.contains("wtransport-work"))
    );
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

const WORKER: &str = "wtransport";

struct ScriptedDocker {
    docker: bollard::Docker,
    path: PathBuf,
    task: Option<tokio::task::JoinHandle<Result<Vec<String>, String>>>,
}

impl ScriptedDocker {
    fn open(responses: Vec<(u16, String)>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-exact-volume-{}-{n}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        let task = tokio::spawn(serve_volume_stub(listener, responses));
        let socket = path
            .to_str()
            .ok_or_else(|| "socket path is not UTF-8".to_owned())?;
        let docker = bollard::Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            docker,
            path,
            task: Some(task),
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
        std::fs::remove_file(&self.path).map_err(|error| error.to_string())?;
        Ok(result)
    }
}

impl Drop for ScriptedDocker {
    fn drop(&mut self) {
        let _aborted = self.task.take().map(|task| task.abort());
        let _kept = std::fs::remove_file(&self.path).err();
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
        stream
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

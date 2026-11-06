// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "runner_log_cleanup/seed.rs"]
mod seed;

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};
use velnor_runner_host::{
    CompletionWorker, HostError, Journal, RepositoryConnection, WorkflowEvent, connect_unix,
};

use super::super::TestResult;
use super::super::runtime_and_policy::sample_connection;

pub(super) const RUNNER_ID: &str =
    "1111111111111111111111111111111111111111111111111111111111111111";
pub(super) const DIND_ID: &str = "2222222222222222222222222222222222222222222222222222222222222222";
pub(super) const PRIVATE_VOLUME: &str = "w0123456789abcdef0123456789abcdef";
pub(super) const RUNNER_IMAGE_ID: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";
pub(super) const RUNNER_IMAGE_REF: &str = "velnor-runner:vaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-1111111111111111111111111111111111111111111111111111111111111111";
pub(super) const DIND_IMAGE_ID: &str =
    "sha256:8888888888888888888888888888888888888888888888888888888888888888";
pub(super) const DIND_IMAGE_REF: &str = "velnor-dind:vaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-9999999999999999999999999999999999999999999999999999999999999999";

pub(super) struct StubObservation {
    pub(super) first: Result<(), HostError>,
    pub(super) replay: Option<Result<(), HostError>>,
    pub(super) runner_present: bool,
    pub(super) exec_creates: usize,
    pub(super) capture_state: Option<Value>,
    pub(super) runner_logs_projection: Option<Value>,
}

struct StubState {
    exit_code: i64,
    archive: Vec<u8>,
    runner_present: bool,
    exec_creates: usize,
    exec_inspected: usize,
}

struct LocalDockerStub {
    stopping: Arc<AtomicBool>,
    dind_stop_entered: Arc<AtomicBool>,
    release_dind_stop: Arc<AtomicBool>,
    state: Arc<Mutex<StubState>>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for LocalDockerStub {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        self.release_dind_stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            drop(handle.join());
        }
    }
}

fn spawn_docker_stub(
    socket_path: &Path,
    exit_code: i64,
    archive: Vec<u8>,
) -> TestResult<LocalDockerStub> {
    let listener = UnixListener::bind(socket_path)?;
    listener.set_nonblocking(true)?;
    let stopping = Arc::new(AtomicBool::new(false));
    let dind_stop_entered = Arc::new(AtomicBool::new(false));
    let release_dind_stop = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(StubState {
        exit_code,
        archive,
        runner_present: true,
        exec_creates: 0,
        exec_inspected: 0,
    }));
    let (thread_stop, thread_entered, thread_release, thread_state) = (
        Arc::clone(&stopping),
        Arc::clone(&dind_stop_entered),
        Arc::clone(&release_dind_stop),
        Arc::clone(&state),
    );
    let handle = thread::spawn(move || {
        while !thread_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let (st, ent, rel) = (
                        Arc::clone(&thread_state),
                        Arc::clone(&thread_entered),
                        Arc::clone(&thread_release),
                    );
                    thread::spawn(move || drop(handle_connection(&mut stream, &st, &ent, &rel)));
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });
    Ok(LocalDockerStub {
        stopping,
        dind_stop_entered,
        release_dind_stop,
        state,
        handle: Some(handle),
    })
}

fn handle_connection(
    stream: &mut UnixStream,
    state: &Arc<Mutex<StubState>>,
    dind_stop_entered: &AtomicBool,
    release_dind_stop: &AtomicBool,
) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let (method, path) = read_http_request(stream)?;
    if method == "POST" && path == format!("containers/{DIND_ID}/stop") {
        dind_stop_entered.store(true, Ordering::Release);
        for _ in 0..400 {
            if release_dind_stop.load(Ordering::Acquire) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        stream.write_all(
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )?;
        return stream.shutdown(std::net::Shutdown::Both);
    }
    let mut guard = state
        .lock()
        .map_err(|_| std::io::Error::other("stub state poisoned"))?;
    write_route_response(stream, &method, &path, &mut guard)
}

fn read_http_request(stream: &mut UnixStream) -> std::io::Result<(String, String)> {
    let mut header = Vec::with_capacity(512);
    let mut byte = [0_u8; 1];
    while !header.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte)?;
        header.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&header);
    let mut lines = text.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let raw_path = parts.next().unwrap_or_default();
    let content_len = lines
        .take_while(|line| !line.is_empty())
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    if content_len > 0 {
        stream.read_exact(&mut vec![0_u8; content_len])?;
    }
    let clean = raw_path.split('?').next().unwrap_or(raw_path);
    let stripped = clean.strip_prefix('/').unwrap_or(clean);
    let api_path = match stripped.split_once('/') {
        Some((ver, rest)) if ver.starts_with('v') => rest.to_owned(),
        _ => stripped.to_owned(),
    };
    Ok((method, api_path))
}

fn container_labels(role: &str) -> Value {
    json!({
        "io.velnor.managed": "true",
        "io.velnor.reservation-id": "2",
        "io.velnor.operation-id": "1",
        "io.velnor.connection-id": "conn-test-1",
        "io.velnor.role": role,
    })
}

fn write_json(
    stream: &mut UnixStream,
    status: u16,
    reason: &str,
    body: &Value,
) -> std::io::Result<()> {
    let payload = serde_json::to_vec(body).unwrap_or_default();
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    )?;
    stream.write_all(&payload)?;
    stream.shutdown(std::net::Shutdown::Both)
}

fn write_route_response(
    stream: &mut UnixStream,
    method: &str,
    path: &str,
    state: &mut StubState,
) -> std::io::Result<()> {
    let info = json!({"ID": "engine-1", "DockerRootDir": "/var/lib/docker", "OSType": "linux", "Architecture": "aarch64"});
    match (method, path) {
        ("GET", "info") => write_json(stream, 200, "OK", &info),
        ("GET", p) if p.starts_with("images/") && p.ends_with("/json") => {
            let (id, tag) = if p.contains(&"4".repeat(64)) {
                (RUNNER_IMAGE_ID, RUNNER_IMAGE_REF)
            } else {
                (DIND_IMAGE_ID, DIND_IMAGE_REF)
            };
            let body = json!({"Id": id, "RepoTags": [tag], "RepoDigests": [], "Os": "linux", "Architecture": "amd64"});
            write_json(stream, 200, "OK", &body)
        }
        ("GET", p) if p == format!("containers/{RUNNER_ID}/json") => {
            if !state.runner_present {
                return write_json(stream, 404, "Not Found", &json!({"message": "missing"}));
            }
            let body = json!({"Id": RUNNER_ID, "Name": format!("/{PRIVATE_VOLUME}-runner"), "Config": {"Labels": container_labels("runner")}, "State": {"Running": false, "ExitCode": 0}});
            write_json(stream, 200, "OK", &body)
        }
        ("DELETE", p) if p == format!("containers/{RUNNER_ID}") => {
            state.runner_present = false;
            stream.write_all(
                b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
            stream.shutdown(std::net::Shutdown::Both)
        }
        ("GET", p) if p == format!("containers/{DIND_ID}/json") => {
            let body = json!({"Id": DIND_ID, "Name": format!("/{PRIVATE_VOLUME}-dind"), "Config": {"Labels": container_labels("dind")}, "Mounts": [{"Type": "volume", "Name": format!("{PRIVATE_VOLUME}-home"), "Destination": "/home/runner", "RW": true}], "State": {"Running": true, "ExitCode": 0}});
            write_json(stream, 200, "OK", &body)
        }
        ("POST", p) if p == format!("containers/{DIND_ID}/exec") => {
            state.exec_creates += 1;
            let body = json!({"Id": format!("{:064x}", state.exec_creates)});
            write_json(stream, 201, "Created", &body)
        }
        _ => write_exec_response(stream, method, path, state),
    }
}

fn write_exec_response(
    stream: &mut UnixStream,
    method: &str,
    path: &str,
    state: &mut StubState,
) -> std::io::Result<()> {
    match (method, path) {
        ("POST", p) if p.starts_with("exec/") && p.ends_with("/start") => {
            stream.write_all(
                b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n",
            )?;
            if !state.archive.is_empty() {
                let len = u32::try_from(state.archive.len()).unwrap_or(0);
                let mut frame = vec![1_u8, 0, 0, 0];
                frame.extend_from_slice(&len.to_be_bytes());
                frame.extend_from_slice(&state.archive);
                stream.write_all(&frame)?;
            }
            stream.shutdown(std::net::Shutdown::Both)
        }
        ("GET", p) if p.starts_with("exec/") && p.ends_with("/json") => {
            let exec_id = p
                .strip_prefix("exec/")
                .and_then(|rest| rest.strip_suffix("/json"))
                .unwrap_or_default();
            state.exec_inspected += 1;
            let body = json!({"ID": exec_id, "ContainerID": DIND_ID, "Running": false, "ExitCode": state.exit_code});
            write_json(stream, 200, "OK", &body)
        }
        _ => write_json(stream, 404, "Not Found", &json!({"message": "unknown"})),
    }
}

async fn run_one_worker_pass(
    ctx: (&Journal, &RepositoryConnection, &Path, &LocalDockerStub),
    docker: bollard::Docker,
) -> TestResult<Result<(), HostError>> {
    let (journal, connection, state_dir, stub) = ctx;
    stub.dind_stop_entered.store(false, Ordering::Release);
    stub.release_dind_stop.store(false, Ordering::Release);
    stub.state
        .lock()
        .map_err(|_| std::io::Error::other("stub state poisoned"))?
        .exec_inspected = 0;
    let worker = CompletionWorker::start_for_connection(
        journal,
        connection,
        docker,
        "https://api.github.com",
        "ghs_test_admin_token".to_owned(),
        "ghp_test_release_read_token",
        state_dir,
    )
    .await?;
    let mut waited_polls = 0_usize;
    while !stub.dind_stop_entered.load(Ordering::Acquire) {
        let bad_exit_done = {
            let g = stub
                .state
                .lock()
                .map_err(|_| std::io::Error::other("stub state poisoned"))?;
            (g.exit_code == 1 && g.exec_inspected >= 3)
                || (!g.archive.is_empty() && g.exit_code != 0 && g.exec_inspected >= 1)
        };
        if bad_exit_done {
            tokio::time::sleep(Duration::from_millis(50)).await;
            break;
        }
        waited_polls += 1;
        if waited_polls > 2_000 {
            return Err("timed out waiting for completion worker pass".into());
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let release = Arc::clone(&stub.release_dind_stop);
    let releaser = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        release.store(true, Ordering::Release);
    });
    let outcome = worker.shutdown().await;
    drop(releaser.await);
    Ok(outcome)
}

pub(super) fn run_runner_log_cleanup_scenario(
    exit_code: i64,
    archive: Vec<u8>,
) -> TestResult<StubObservation> {
    let temp = tempfile::tempdir()?;
    let state_dir = temp.path().canonicalize()?;
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700))?;
    let socket_path = state_dir.join("docker.sock");
    let stub = spawn_docker_stub(&socket_path, exit_code, archive)?;
    let connection = sample_connection(vec![WorkflowEvent::Push], Vec::new())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (first, replay) = runtime.block_on(async {
        let journal =
            seed::seed_completed_launch(&state_dir.join("journal.db"), &connection).await?;
        let docker = connect_unix(socket_path.to_str().ok_or("socket utf8")?)?;
        let ctx = (&journal, &connection, state_dir.as_path(), &stub);
        let first = run_one_worker_pass(ctx, docker.clone()).await?;
        let replay = if first.is_ok() {
            Some(run_one_worker_pass(ctx, docker).await?)
        } else {
            None
        };
        Ok::<_, Box<dyn std::error::Error>>((first, replay))
    })?;
    let guard = stub.state.lock().map_err(|_| "stub state poisoned")?;
    let (capture_state, runner_logs_projection) = read_diagnostics_files(&state_dir)?;
    Ok(StubObservation {
        first,
        replay,
        runner_present: guard.runner_present,
        exec_creates: guard.exec_creates,
        capture_state,
        runner_logs_projection,
    })
}

fn read_opt_json(path: &Path) -> TestResult<Option<Value>> {
    path.exists()
        .then(|| Ok(serde_json::from_slice(&std::fs::read(path)?)?))
        .transpose()
}

fn read_diagnostics_files(state_dir: &Path) -> TestResult<(Option<Value>, Option<Value>)> {
    for entry in std::fs::read_dir(state_dir.join("completion-diagnostics"))
        .into_iter()
        .flatten()
    {
        let path = entry?.path();
        if path.is_dir() {
            return Ok((
                read_opt_json(&path.join("i1-r2-runner-log-state.json"))?,
                read_opt_json(&path.join("i1-g1-runner-logs.json"))?,
            ));
        }
    }
    Ok((None, None))
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
    HostError, Journal, RepositoryConnection, WorkflowEvent, connect_unix,
    remove_completed_runner_container,
};

use super::super::TestResult;
use super::super::runtime_and_policy::sample_connection;

const RUNNER_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const DIND_ID: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const PRIVATE_VOLUME: &str = "w0123456789abcdef0123456789abcdef";

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
}

struct LocalDockerStub {
    stopping: Arc<AtomicBool>,
    state: Arc<Mutex<StubState>>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for LocalDockerStub {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
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
    let state = Arc::new(Mutex::new(StubState {
        exit_code,
        archive,
        runner_present: true,
        exec_creates: 0,
    }));
    let thread_stop = Arc::clone(&stopping);
    let thread_state = Arc::clone(&state);
    let handle = thread::spawn(move || {
        let mut workers = Vec::new();
        while !thread_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let conn_state = Arc::clone(&thread_state);
                    workers.push(thread::spawn(move || {
                        drop(handle_connection(&mut stream, &conn_state));
                    }));
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
        for worker in workers {
            drop(worker.join());
        }
    });
    Ok(LocalDockerStub {
        stopping,
        state,
        handle: Some(handle),
    })
}

fn handle_connection(
    stream: &mut UnixStream,
    state: &Arc<Mutex<StubState>>,
) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let (method, path) = read_http_request(stream)?;
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
    match (method, path) {
        ("GET", "info") => write_json(
            stream,
            200,
            "OK",
            &json!({"ID": "engine-1", "DockerRootDir": "/var/lib/docker", "OSType": "linux", "Architecture": "aarch64"}),
        ),
        ("GET", p) if p == format!("containers/{RUNNER_ID}/json") => {
            if !state.runner_present {
                return write_json(stream, 404, "Not Found", &json!({"message": "missing"}));
            }
            write_json(
                stream,
                200,
                "OK",
                &json!({
                    "Id": RUNNER_ID,
                    "Name": format!("/{PRIVATE_VOLUME}-runner"),
                    "Config": {"Labels": container_labels("runner")},
                    "State": {"Running": false, "ExitCode": 0}
                }),
            )
        }
        ("GET", p) if p == format!("containers/{RUNNER_ID}/logs") => {
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/vnd.docker.raw-stream\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
            stream.shutdown(std::net::Shutdown::Both)
        }
        ("DELETE", p) if p == format!("containers/{RUNNER_ID}") => {
            state.runner_present = false;
            stream.write_all(
                b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
            stream.shutdown(std::net::Shutdown::Both)
        }
        ("GET", p) if p == format!("containers/{DIND_ID}/json") => write_json(
            stream,
            200,
            "OK",
            &json!({
                "Id": DIND_ID,
                "Name": format!("/{PRIVATE_VOLUME}-dind"),
                "Config": {"Labels": container_labels("dind")},
                "Mounts": [{"Type": "volume", "Name": format!("{PRIVATE_VOLUME}-home"), "Destination": "/home/runner", "RW": true}],
                "State": {"Running": true, "ExitCode": 0}
            }),
        ),
        ("POST", p) if p == format!("containers/{DIND_ID}/exec") => {
            state.exec_creates += 1;
            write_json(
                stream,
                201,
                "Created",
                &json!({"Id": format!("{:064x}", state.exec_creates)}),
            )
        }
        _ => write_exec_response(stream, method, path, state),
    }
}

fn write_exec_response(
    stream: &mut UnixStream,
    method: &str,
    path: &str,
    state: &StubState,
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
            write_json(
                stream,
                200,
                "OK",
                &json!({"ID": exec_id, "ContainerID": DIND_ID, "Running": false, "ExitCode": state.exit_code}),
            )
        }
        _ => write_json(stream, 404, "Not Found", &json!({"message": "unknown"})),
    }
}

async fn seed_completed_launch(
    journal_path: &Path,
    connection: &RepositoryConnection,
) -> TestResult<()> {
    let identity = String::from_utf8(connection.canonical_identity_bytes()?)?;
    let db = turso::Builder::new_local(
        journal_path
            .to_str()
            .ok_or_else(|| std::io::Error::other("journal path utf8"))?,
    )
    .build()
    .await?;
    let conn = db.connect()?;
    conn.execute(
        "INSERT INTO connection_scopes (connection_id, identity) VALUES (?1, ?2)",
        ("conn-test-1", identity.as_str()),
    )
    .await?;
    conn.execute(
        "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES ('conn-test-1', 7, 'ubuntu-26.04-scale-set', 'created', 'engine-1', 'unix:///docker.sock', 'linux/arm64')",
        (),
    )
    .await?;
    conn.execute(
        "INSERT INTO connection_intents (id, connection_id, kind, subject, state, cleanup_proven, docker_id, dind_id, worker_volume, scale_set_id, runner_request_id, runner_name) VALUES (1, 'conn-test-1', 'launch', 'req-11', 'done', 0, ?1, ?2, ?3, 7, 11, 'v0123456789abcdef0123456789abcdef')",
        (RUNNER_ID, DIND_ID, PRIVATE_VOLUME),
    )
    .await?;
    conn.execute(
        "INSERT INTO connection_completion_cleanup (intent_id, connection_id, scale_set_id, runner_request_id, runner_id, runner_name, runner_absent) VALUES (1, 'conn-test-1', 7, 11, 21, 'v0123456789abcdef0123456789abcdef', 1)",
        (),
    )
    .await?;
    Ok(())
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
        let journal_path = state_dir.join("journal.db");
        let journal = Journal::open(&journal_path).await?;
        seed_completed_launch(&journal_path, &connection).await?;
        let docker = connect_unix(
            socket_path
                .to_str()
                .ok_or_else(|| std::io::Error::other("socket utf8"))?,
        )?;
        let first =
            remove_completed_runner_container(&docker, &journal, &connection, &state_dir, 2).await;
        let replay = if first.is_ok() {
            Some(
                remove_completed_runner_container(&docker, &journal, &connection, &state_dir, 2)
                    .await,
            )
        } else {
            None
        };
        Ok::<_, Box<dyn std::error::Error>>((first, replay))
    })?;
    let guard = stub
        .state
        .lock()
        .map_err(|_| std::io::Error::other("stub state poisoned"))?;
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
    if path.exists() {
        Ok(Some(serde_json::from_slice(&std::fs::read(path)?)?))
    } else {
        Ok(None)
    }
}

fn read_diagnostics_files(state_dir: &Path) -> TestResult<(Option<Value>, Option<Value>)> {
    let root = state_dir.join("completion-diagnostics");
    if !root.exists() {
        return Ok((None, None));
    }
    for entry in std::fs::read_dir(&root)? {
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

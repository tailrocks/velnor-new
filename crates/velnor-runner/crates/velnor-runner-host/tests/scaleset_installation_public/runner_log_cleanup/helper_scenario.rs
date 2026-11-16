// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::{Value, json};
use velnor_runner_host::{CompletionWorker, connect_unix};

use super::super::super::TestResult;
use super::super::super::runtime_and_policy::sample_connection;
use super::{DIND_ID, PRIVATE_VOLUME, WorkflowEvent, seed, spawn_docker_stub, write_json};

pub(super) const HELPER_ID: &str =
    "3333333333333333333333333333333333333333333333333333333333333333";
const ROLES: [(&str, &str); 4] = [
    ("", "shared-run"),
    ("-home", "runner-home"),
    ("-work", "runner-work"),
    ("-docker", "dind-data"),
];

#[derive(Default)]
pub(super) struct VolumeState {
    pub(super) volumes: Vec<String>,
    pub(super) dind_removed: bool,
    pub(crate) helper_present: bool,
}

impl VolumeState {
    fn cancelled_before_start() -> Self {
        Self {
            volumes: ROLES
                .iter()
                .map(|(suffix, _)| format!("{PRIVATE_VOLUME}{suffix}"))
                .collect(),
            helper_present: true,
            dind_removed: false,
        }
    }
}

fn no_content(stream: &mut UnixStream) -> std::io::Result<()> {
    stream
        .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
    stream.shutdown(std::net::Shutdown::Both)
}

pub(super) fn write_volume_response(
    stream: &mut UnixStream,
    method: &str,
    path: &str,
    state: &mut VolumeState,
) -> std::io::Result<()> {
    let missing = json!({"message": "No such object"});
    match (method, path.split('/').collect::<Vec<_>>().as_slice()) {
        ("GET", ["containers", "json"]) => {
            let listed = if state.helper_present {
                json!([{"Id": HELPER_ID}])
            } else {
                json!([])
            };
            write_json(stream, 200, "OK", &listed)
        }
        ("GET", ["containers", id, "json"]) if *id == HELPER_ID && state.helper_present => {
            let mounts: Vec<Value> = state
                .volumes
                .iter()
                .map(|name| json!({"Type": "volume", "Name": name, "Destination": "/opt/velnor", "RW": true}))
                .collect();
            let body = json!({"Id": HELPER_ID, "Name": "/velnor-execution-publication-op", "Config": {"Labels": {"velnor.role": "execution-publication"}}, "Mounts": mounts, "State": {"Running": false, "Status": "created"}});
            write_json(stream, 200, "OK", &body)
        }
        ("DELETE", ["containers", id]) if *id == DIND_ID => {
            state.dind_removed = true;
            no_content(stream)
        }
        ("DELETE", ["containers", id]) if *id == HELPER_ID && state.helper_present => {
            state.helper_present = false;
            no_content(stream)
        }
        ("GET", ["volumes", name]) => {
            let role = ROLES
                .iter()
                .find(|(suffix, _)| format!("{PRIVATE_VOLUME}{suffix}") == *name)
                .filter(|_| state.volumes.iter().any(|volume| volume == name))
                .map(|(_, role)| *role);
            match role {
                Some(role) => write_json(
                    stream,
                    200,
                    "OK",
                    &json!({"Name": name, "Driver": "local", "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"), "Labels": {"velnor.connection": "conn-test-1", "velnor.reservation": "2", "velnor.operation": "1", "velnor.role": role}, "Scope": "local", "Options": {}}),
                ),
                None => write_json(stream, 404, "Not Found", &missing),
            }
        }
        ("DELETE", ["volumes", name]) if state.helper_present => write_json(
            stream,
            409,
            "Conflict",
            &json!({"message": format!("remove {name}: volume is in use - [{HELPER_ID}]")}),
        ),
        ("DELETE", ["volumes", name]) => {
            state.volumes.retain(|volume| volume != name);
            no_content(stream)
        }
        _ => write_json(stream, 404, "Not Found", &missing),
    }
}

pub(crate) struct CancelledObservation {
    pub(crate) helper_present: bool,
    pub(crate) volumes_left: usize,
}

pub(crate) fn run_cancelled_before_start_scenario() -> TestResult<CancelledObservation> {
    let temp = tempfile::tempdir()?;
    let state_dir = temp.path().canonicalize()?;
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700))?;
    let socket_path = state_dir.join("docker.sock");
    let stub = spawn_docker_stub(&socket_path, 3, Vec::new())?;
    stub.release_dind_stop.store(true, Ordering::Release);
    stub.state
        .lock()
        .map_err(|_| "stub state poisoned")?
        .volumes = VolumeState::cancelled_before_start();
    let connection = sample_connection(vec![WorkflowEvent::Push], Vec::new())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let journal =
            seed::seed_completed_launch(&state_dir.join("journal.db"), &connection).await?;
        let docker = connect_unix(socket_path.to_str().ok_or("socket utf8")?)?;
        let worker = CompletionWorker::start_for_connection(
            &journal,
            &connection,
            docker,
            "https://api.github.com",
            "ghs_test_admin_token".to_owned(),
            "ghp_test_release_read_token",
            &state_dir,
        )
        .await?;
        for _ in 0..750 {
            let done = {
                let guard = stub.state.lock().map_err(|_| "stub state poisoned")?;
                !guard.volumes.helper_present && guard.volumes.volumes.is_empty()
            };
            if done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let _outcome = worker.shutdown().await;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    let guard = stub.state.lock().map_err(|_| "stub state poisoned")?;
    Ok(CancelledObservation {
        helper_present: guard.volumes.helper_present,
        volumes_left: guard.volumes.volumes.len(),
    })
}

pub(super) fn read_diagnostics_files(
    state_dir: &Path,
) -> TestResult<(Option<Value>, Option<Value>)> {
    fn read_opt_json(path: &Path) -> TestResult<Option<Value>> {
        path.exists()
            .then(|| Ok(serde_json::from_slice(&std::fs::read(path)?)?))
            .transpose()
    }
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

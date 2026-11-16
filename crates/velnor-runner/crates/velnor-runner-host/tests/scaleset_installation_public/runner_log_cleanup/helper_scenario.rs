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

pub(super) const ENGINE_DIGEST: &str =
    "b52bdf3b3388b9a5564e3c9873ed2a2c4ed4a5af1078ab1b1445e94c3dd0114f";
const ROLES: [(&str, &str); 4] = [
    ("", "shared-run"),
    ("-home", "runner-home"),
    ("-work", "runner-work"),
    ("-docker", "dind-data"),
];

#[derive(Clone)]
pub(crate) struct HelperSpec {
    pub(crate) role: &'static str,
    pub(crate) mounts: bool,
    pub(crate) listed: bool,
    pub(crate) running: bool,
    pub(crate) engine: Option<&'static str>,
}

impl HelperSpec {
    pub(crate) const fn probe() -> Self {
        Self {
            role: "resource-probe",
            mounts: true,
            listed: true,
            running: false,
            engine: Some(ENGINE_DIGEST),
        }
    }

    pub(crate) const fn with_role(role: &'static str, engine: Option<&'static str>) -> Self {
        Self {
            role,
            engine,
            ..Self::probe()
        }
    }
}

struct Helper {
    id: String,
    spec: HelperSpec,
    deleted: bool,
}

#[derive(Default)]
pub(super) struct VolumeState {
    pub(super) volumes: Vec<String>,
    pub(super) dind_removed: bool,
    helpers: Vec<Helper>,
    stops: Vec<String>,
    list_calls: usize,
}

impl VolumeState {
    fn cancelled_before_start(specs: &[HelperSpec]) -> Self {
        Self {
            volumes: ROLES
                .iter()
                .map(|(suffix, _)| format!("{PRIVATE_VOLUME}{suffix}"))
                .collect(),
            helpers: specs
                .iter()
                .enumerate()
                .map(|(index, spec)| Helper {
                    id: format!("{index}{}", "3".repeat(63)),
                    spec: spec.clone(),
                    deleted: false,
                })
                .collect(),
            ..Self::default()
        }
    }

    fn present(&self) -> impl Iterator<Item = &Helper> {
        self.helpers.iter().filter(|helper| !helper.deleted)
    }

    fn helper_mut(&mut self, id: &str) -> Option<&mut Helper> {
        self.helpers
            .iter_mut()
            .find(|helper| !helper.deleted && helper.id == id)
    }
}

fn no_content(stream: &mut UnixStream) -> std::io::Result<()> {
    stream
        .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
    stream.shutdown(std::net::Shutdown::Both)
}

fn helper_body(helper: &Helper, volumes: &[String]) -> Value {
    let mounted: Vec<String> = if helper.spec.mounts {
        volumes.to_vec()
    } else {
        vec!["unrelated-volume".to_owned()]
    };
    let mounts: Vec<Value> = mounted
        .iter()
        .map(|name| json!({"Type": "volume", "Name": name, "Destination": "/opt/velnor", "RW": true}))
        .collect();
    let mut labels = json!({"velnor.role": helper.spec.role});
    if let Some(engine) = helper.spec.engine {
        labels["velnor.engine"] = json!(engine);
    }
    json!({"Id": helper.id, "Name": format!("/velnor-{}-op", helper.spec.role), "Config": {"Labels": labels}, "Mounts": mounts, "State": {"Running": helper.spec.running, "Status": "created"}})
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
            state.list_calls += 1;
            let listed: Vec<Value> = state
                .present()
                .filter(|helper| helper.spec.listed)
                .map(|helper| json!({"Id": helper.id}))
                .collect();
            write_json(stream, 200, "OK", &Value::Array(listed))
        }
        ("GET", ["containers", id, "json"]) if state.present().any(|h| h.id == *id) => {
            let body = state
                .present()
                .find(|helper| helper.id == *id)
                .map(|helper| helper_body(helper, &state.volumes));
            write_json(stream, 200, "OK", &body.unwrap_or(missing))
        }
        ("POST", ["containers", id, "stop"]) if state.present().any(|h| h.id == *id) => {
            state.stops.push((*id).to_owned());
            if let Some(helper) = state.helper_mut(id) {
                helper.spec.running = false;
            }
            no_content(stream)
        }
        ("DELETE", ["containers", id]) if *id == DIND_ID => {
            state.dind_removed = true;
            no_content(stream)
        }
        ("DELETE", ["containers", id]) if state.present().any(|h| h.id == *id) => {
            match state.helper_mut(id) {
                Some(helper) if helper.spec.running => write_json(
                    stream,
                    409,
                    "Conflict",
                    &json!({"message": "cannot remove a running container"}),
                ),
                Some(helper) => {
                    helper.deleted = true;
                    no_content(stream)
                }
                None => write_json(stream, 404, "Not Found", &missing),
            }
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
        ("DELETE", ["volumes", name]) if state.present().any(|helper| helper.spec.mounts) => {
            write_json(
                stream,
                409,
                "Conflict",
                &json!({"message": format!("remove {name}: volume is in use")}),
            )
        }
        ("DELETE", ["volumes", name]) => {
            state.volumes.retain(|volume| volume != name);
            no_content(stream)
        }
        _ => write_json(stream, 404, "Not Found", &missing),
    }
}

pub(crate) struct CancelledObservation {
    pub(crate) helpers_left: usize,
    pub(crate) unmounted_left: usize,
    pub(crate) stops: usize,
    pub(crate) volumes_left: usize,
}

impl CancelledObservation {
    pub(crate) fn encode(&self) -> String {
        format!(
            "OBS {} {} {} {}",
            self.helpers_left, self.unmounted_left, self.stops, self.volumes_left
        )
    }

    pub(crate) fn decode(stdout: &str) -> Option<Self> {
        let line = stdout
            .lines()
            .find_map(|line| line.split_once("OBS "))
            .map(|(_, rest)| rest)?;
        let mut fields = line.split(' ').map(|field| field.parse::<usize>().ok());
        Some(Self {
            helpers_left: fields.next()??,
            unmounted_left: fields.next()??,
            stops: fields.next()??,
            volumes_left: fields.next()??,
        })
    }
}

pub(crate) fn run_cancelled_before_start_scenario(
    specs: &[HelperSpec],
    expect_success: bool,
) -> TestResult<CancelledObservation> {
    let temp = tempfile::tempdir()?;
    let state_dir = temp.path().canonicalize()?;
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700))?;
    let socket_path = state_dir.join("docker.sock");
    let stub = spawn_docker_stub(&socket_path, 3, Vec::new())?;
    stub.release_dind_stop.store(true, Ordering::Release);
    stub.state
        .lock()
        .map_err(|_| "stub state poisoned")?
        .volumes = VolumeState::cancelled_before_start(specs);
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
                let volumes = &guard.volumes;
                if expect_success {
                    volumes.present().all(|helper| !helper.spec.mounts)
                        && volumes.volumes.is_empty()
                } else {
                    volumes.list_calls > 0
                }
            };
            if done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if !expect_success {
            tokio::time::sleep(Duration::from_millis(600)).await;
        }
        let _outcome = worker.shutdown().await;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    let guard = stub.state.lock().map_err(|_| "stub state poisoned")?;
    let volumes = &guard.volumes;
    Ok(CancelledObservation {
        helpers_left: volumes
            .present()
            .filter(|helper| helper.spec.mounts)
            .count(),
        unmounted_left: volumes
            .present()
            .filter(|helper| !helper.spec.mounts)
            .count(),
        stops: volumes.stops.len(),
        volumes_left: volumes.volumes.len(),
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

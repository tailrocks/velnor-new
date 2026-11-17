// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;

use super::TestResult;
use super::seed::{CONNECTION_ID, ENGINE_ID, Pair};

struct Container {
    name: String,
    labels: Value,
    running: bool,
}

#[derive(Default)]
struct State {
    containers: BTreeMap<String, Container>,
    volumes: BTreeMap<String, Value>,
    networks: BTreeMap<String, (String, Value)>,
    requests: Vec<String>,
    hang_after: Option<String>,
    armed: bool,
    read_gate: Option<String>,
    read_gate_hits: usize,
}

pub(crate) struct FixtureDocker {
    state: Arc<Mutex<State>>,
    hung: Arc<Notify>,
}

impl FixtureDocker {
    pub(crate) fn start(socket: &Path, pairs: &[Pair]) -> TestResult<Self> {
        let listener = UnixListener::bind(socket)?;
        let mut state = State::default();
        for pair in pairs {
            let (reservation, operation) = (
                pair.reservation.to_string(),
                (pair.reservation - 1).to_string(),
            );
            for role in ["runner", "dind"] {
                state.containers.insert(
                    pair.container_id(role),
                    Container {
                        name: format!("w{}-{role}", pair.nonce()),
                        labels: json!({"io.velnor.managed": "true", "io.velnor.reservation-id": reservation, "io.velnor.operation-id": operation, "io.velnor.connection-id": CONNECTION_ID, "io.velnor.role": role}),
                        running: true,
                    },
                );
            }
            for (name, role) in pair.volumes() {
                state.volumes.insert(
                    name,
                    json!({"velnor.connection": CONNECTION_ID, "velnor.reservation": reservation, "velnor.operation": operation, "velnor.role": role}),
                );
            }
            state.networks.insert(
                pair.network_id(),
                (
                    format!("velnor-v{}-private", pair.nonce()),
                    json!({"io.velnor.connection-id": CONNECTION_ID, "io.velnor.engine-id": ENGINE_ID, "io.velnor.managed": "true", "io.velnor.operation-id": operation, "io.velnor.reservation-id": reservation}),
                ),
            );
        }
        let fixture = Self {
            state: Arc::new(Mutex::new(state)),
            hung: Arc::default(),
        };
        let (state, hung) = (Arc::clone(&fixture.state), Arc::clone(&fixture.hung));
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (state, hung) = (Arc::clone(&state), Arc::clone(&hung));
                tokio::spawn(async move { drop(serve(stream, &state, &hung).await) });
            }
        });
        Ok(fixture)
    }

    pub(crate) fn add_initializer(&self, pair: Pair) {
        lock(&self.state).containers.insert(
            pair.helper_id(),
            Container {
                name: pair.helper_name(),
                labels: json!({"velnor.role": "storage-initializer", "velnor.operation": format!("{}-1", pair.reservation - 1)}),
                running: false,
            },
        );
    }

    pub(crate) fn relabel_initializer(&self, pair: Pair) {
        if let Some(container) = lock(&self.state).containers.get_mut(&pair.helper_id()) {
            container.labels["velnor.operation"] = json!("999-1");
        }
    }

    pub(crate) fn has_initializer(&self, pair: Pair) -> bool {
        lock(&self.state).containers.contains_key(&pair.helper_id())
    }

    pub(crate) fn hang_after(&self, request: String) {
        lock(&self.state).hang_after = Some(request);
    }

    pub(crate) fn gate_container_inspection(&self, id: &str) {
        lock(&self.state).read_gate = Some(format!("GET /containers/{id}/json"));
    }

    pub(crate) fn inspection_gate_hits(&self) -> usize {
        lock(&self.state).read_gate_hits
    }

    pub(crate) async fn hung(&self) {
        self.hung.notified().await;
    }

    pub(crate) fn relabel_container(&self, id: &str) {
        if let Some(container) = lock(&self.state).containers.get_mut(id) {
            container.labels["io.velnor.reservation-id"] = json!("999");
        }
    }

    pub(crate) fn rename_container(&self, id: &str, name: &str) {
        if let Some(container) = lock(&self.state).containers.get_mut(id) {
            name.clone_into(&mut container.name);
        }
    }

    pub(crate) fn reissue_container(&self, id: &str, new_id: &str) {
        let mut state = lock(&self.state);
        if let Some(container) = state.containers.remove(id) {
            state.containers.insert(new_id.to_owned(), container);
        }
    }

    pub(crate) fn relabel_volume(&self, name: &str) {
        if let Some(labels) = lock(&self.state).volumes.get_mut(name) {
            labels["velnor.reservation"] = json!("999");
        }
    }

    pub(crate) fn has_container(&self, id: &str) -> bool {
        lock(&self.state).containers.contains_key(id)
    }

    pub(crate) fn take_requests(&self) -> Vec<String> {
        std::mem::take(&mut lock(&self.state).requests)
    }

    pub(crate) fn objects(&self, pair: Pair) -> Vec<String> {
        let state = lock(&self.state);
        let containers = ["runner", "dind"]
            .map(|role| pair.container_id(role))
            .into_iter()
            .filter(|id| state.containers.contains_key(id));
        let volumes = pair
            .volumes()
            .map(|(name, _)| name)
            .into_iter()
            .filter(|name| state.volumes.contains_key(name));
        let networks = [pair.network_id()]
            .into_iter()
            .filter(|id| state.networks.contains_key(id));
        containers.chain(volumes).chain(networks).collect()
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn respond(state: &mut State, method: &str, segments: &[&str]) -> (u16, Value) {
    let found = |value: Option<Value>| {
        value.map_or((404, json!({"message": "No such object"})), |value| {
            (200, value)
        })
    };
    match (method, segments) {
        ("GET", ["info"]) => (200, json!({"ID": ENGINE_ID, "DockerRootDir": "/var/lib/docker", "OSType": "linux", "Architecture": "x86_64"})),
        ("GET", ["containers", key, "json"]) => found(
            state
                .containers
                .iter()
                .find(|(id, container)| *id == key || container.name == *key)
                .map(|(id, container)| json!({"Id": id, "Name": format!("/{}", container.name), "State": {"Running": container.running}, "Config": {"Labels": container.labels}})),
        ),
        ("POST", ["containers", id, "stop"]) => match state.containers.get_mut(*id) {
            Some(container) => {
                container.running = false;
                (204, Value::Null)
            }
            None => found(None),
        },
        ("DELETE", ["containers", id]) => match state.containers.get(*id) {
            Some(container) if container.running => (409, json!({"message": "running"})),
            Some(_) => {
                state.containers.remove(*id);
                (204, Value::Null)
            }
            None => found(None),
        },
        ("GET", ["volumes", name]) => found(state.volumes.get(*name).map(|labels| {
            json!({"Name": name, "Driver": "local", "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"), "Labels": labels, "Scope": "local", "Options": {}})
        })),
        ("DELETE", ["volumes", name]) => match state.volumes.remove(*name) {
            Some(_) => (204, Value::Null),
            None => found(None),
        },
        ("GET", ["networks", key]) => found(
            state
                .networks
                .iter()
                .find(|(id, (name, _))| *id == key || name == key)
                .map(|(id, (name, labels))| json!({"Name": name, "Id": id, "Driver": "bridge", "Scope": "local", "Internal": false, "Attachable": false, "Ingress": false, "EnableIPv4": true, "EnableIPv6": false, "Containers": {}, "Labels": labels})),
        ),
        ("DELETE", ["networks", id]) => match state.networks.remove(*id) {
            Some(_) => (204, Value::Null),
            None => found(None),
        },
        _ => found(None),
    }
}

async fn serve(mut stream: UnixStream, state: &Mutex<State>, hung: &Notify) -> TestResult<()> {
    let (method, target) = read_request(&mut stream).await?;
    let route = target
        .split_once('?')
        .map_or(target.as_str(), |(route, _)| route);
    let request = format!("{method} {route}");
    let segments: Vec<&str> = route.split('/').skip(1).collect();
    let response = {
        let mut state = lock(state);
        if state.read_gate.as_deref() == Some(request.as_str()) {
            state.requests.push(request.clone());
            state.read_gate_hits += 1;
            None
        } else if state.armed {
            state.armed = false;
            state.hang_after = None;
            None
        } else {
            state.requests.push(request.clone());
            state.armed = state.hang_after.as_deref() == Some(request.as_str());
            Some(respond(&mut state, &method, &segments))
        }
    };
    let Some((status, body)) = response else {
        hung.notify_one();
        return std::future::pending().await;
    };
    let body = if body.is_null() {
        String::new()
    } else {
        body.to_string()
    };
    let head = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    Ok(stream.shutdown().await?)
}

async fn read_request(stream: &mut UnixStream) -> TestResult<(String, String)> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(head_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&request[..head_end]).into_owned();
            let mut words = head.split_whitespace().map(str::to_owned);
            return Ok((
                words.next().unwrap_or_default(),
                words.next().unwrap_or_default(),
            ));
        }
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err("the request closed early".into());
        }
        request.extend_from_slice(&chunk[..read]);
    }
}

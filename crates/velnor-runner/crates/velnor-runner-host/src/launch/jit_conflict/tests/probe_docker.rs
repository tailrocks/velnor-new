// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::TestResult;
use super::seed::{CONNECTION_ID, ENGINE_ID, Pair};

#[path = "probe_wire.rs"]
mod wire;

struct Container {
    name: String,
    labels: Value,
    running: bool,
    config: Option<Value>,
    exited: bool,
}

#[derive(Default)]
struct State {
    containers: BTreeMap<String, Container>,
    volumes: BTreeMap<String, Value>,
    networks: BTreeMap<String, (String, Value)>,
    requests: Vec<String>,
    next_id: u64,
}

pub(crate) struct FixtureDocker {
    state: Arc<Mutex<State>>,
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
                        config: None,
                        exited: false,
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
        };
        let state = Arc::clone(&fixture.state);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let state = Arc::clone(&state);
                tokio::spawn(async move { drop(serve(stream, &state).await) });
            }
        });
        Ok(fixture)
    }

    pub(crate) fn relabel_container(&self, id: &str) {
        if let Some(container) = lock(&self.state).containers.get_mut(id) {
            container.labels["io.velnor.reservation-id"] = json!("999");
        }
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

fn respond(
    state: &mut State,
    method: &str,
    segments: &[&str],
    target: &str,
    body: &[u8],
) -> TestResult<(u16, Value)> {
    let found = |value: Option<Value>| {
        value.map_or((404, json!({"message": "No such object"})), |value| {
            (200, value)
        })
    };
    let response = match (method, segments) {
        ("GET", ["info"]) => (200, json!({"ID": ENGINE_ID, "DockerRootDir": "/var/lib/docker", "OSType": "linux", "Architecture": "x86_64", "NCPU": 4, "MemTotal": 8_589_934_592_u64})),
        ("GET", ["containers", key, "json"]) => found(
            state
                .containers
                .iter()
                .find(|(id, container)| *id == key || container.name == *key)
                .map(|(id, container)| wire::inspect(id, &container.name, &container.labels, container.running, container.exited, container.config.as_ref())).transpose()?,
        ),
        ("GET", ["images", id, "json"]) => (200, wire::image(id)),
        ("POST", ["containers", "create"]) => {
            let config: Value = serde_json::from_slice(body)?;
            let name = target.split_once("name=").ok_or("container name")?.1.split('&').next().ok_or("name")?.to_owned();
            state.next_id += 1;
            let id = format!("{:064x}", state.next_id + 100);
            state.containers.insert(id.clone(), Container { name, labels: config["Labels"].clone(), running: false, config: Some(config), exited: false });
            (201, json!({"Id": id, "Warnings": []}))
        }
        ("POST", ["containers", id, "start"]) => {
            let container = state.containers.get_mut(*id).ok_or("created probe")?;
            container.running = true;
            (204, Value::Null)
        }
        ("POST", ["containers", id, "wait"]) => {
            let container = state.containers.get_mut(*id).ok_or("started probe")?;
            container.running = false;
            container.exited = true;
            (200, json!({"StatusCode": 0}))
        }
        ("POST", ["containers", id, "stop"])
 => match state.containers.get_mut(*id) {
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
    };
    Ok(response)
}

async fn serve(mut stream: UnixStream, state: &Mutex<State>) -> TestResult<()> {
    let (method, target, body) = read_request(&mut stream).await?;
    let route = target
        .split_once('?')
        .map_or(target.as_str(), |(route, _)| route);
    let request = format!("{method} {target}");
    lock(state).requests.push(request);
    let segments: Vec<&str> = route.split('/').skip(1).collect();
    if let ["containers", id, "attach"] = segments.as_slice() {
        stream
            .write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")
            .await?;
        let mut input = Vec::new();
        let mut chunk = [0_u8; 4096];
        while input.split(|byte| *byte == b'\n').nth(2).is_none() {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Err("observer input closed before acknowledgement".into());
            }
            input.extend_from_slice(&chunk[..read]);
        }
        let output = wire::observer(&input)?;
        if let Some(container) = lock(state).containers.get_mut(*id) {
            container.running = false;
            container.exited = true;
        }
        let frame = wire::frame(&output)?;
        stream.write_all(&frame).await?;
        return Ok(stream.shutdown().await?);
    }
    if let ["containers", _, "logs"] = segments.as_slice() {
        let output = wire::frame(&wire::capacity()?)?;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    output.len()
                )
                .as_bytes(),
            )
            .await?;
        stream.write_all(&output).await?;
        return Ok(stream.shutdown().await?);
    }
    let (status, body) = respond(&mut lock(state), &method, &segments, &target, &body)?;
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

async fn read_request(stream: &mut UnixStream) -> TestResult<(String, String, Vec<u8>)> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(head_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&request[..head_end]).into_owned();
            let length = head
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if request.len() >= head_end + 4 + length {
                let mut words = head.split_whitespace().map(str::to_owned);
                return Ok((
                    words.next().unwrap_or_default(),
                    words.next().unwrap_or_default(),
                    request[head_end + 4..head_end + 4 + length].to_vec(),
                ));
            }
        }
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err("the request closed early".into());
        }
        request.extend_from_slice(&chunk[..read]);
    }
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;

use super::TestResult;
use super::seed::{CONNECTION_ID, ENGINE_ID, IMAGE_ID, PRIVATE_VOLUME};

pub(super) const HELPER_ID: &str =
    "c0ffee0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
const IMAGE_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

#[derive(Default)]
struct State {
    helper: Option<(String, Value)>,
    requests: Vec<String>,
    hang_on: Option<String>,
    removed_volumes: Vec<String>,
}

pub(super) struct FixtureDocker {
    state: Arc<Mutex<State>>,
    hung: Arc<Notify>,
}

impl FixtureDocker {
    pub(super) fn start(socket: &Path, hang_on: String) -> TestResult<Self> {
        let listener = UnixListener::bind(socket)?;
        let fixture = Self {
            state: Arc::new(Mutex::new(State {
                hang_on: Some(hang_on),
                ..State::default()
            })),
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

    pub(super) async fn hung(&self) {
        self.hung.notified().await;
    }

    pub(super) fn helper_present(&self) -> bool {
        lock(&self.state).helper.is_some()
    }

    pub(super) fn take_requests(&self) -> Vec<String> {
        std::mem::take(&mut lock(&self.state).requests)
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn serve(mut stream: UnixStream, state: &Mutex<State>, hung: &Notify) -> TestResult<()> {
    let (method, target, body) = read_request(&mut stream).await?;
    let (route, query) = target.split_once('?').unwrap_or((&target, ""));
    let route = route.replace("%3A", ":");
    let request = format!("{method} {route}");
    let killed_here = {
        let mut state = lock(state);
        state.requests.push(request.clone());
        let matched = state.hang_on.as_deref() == Some(request.as_str());
        if matched {
            state.hang_on = None;
        }
        matched
    };
    let segments: Vec<&str> = route.split('/').skip(1).collect();
    let response = match (method.as_str(), segments.as_slice()) {
        ("GET", ["info"]) => Some(
            json!({"ID": ENGINE_ID, "DockerRootDir": "/var/lib/docker", "OSType": "linux", "Architecture": "x86_64"}),
        ),
        ("GET", ["images", IMAGE_ID, "json"]) => Some(
            json!({"Id": IMAGE_ID, "Os": "linux", "Architecture": "amd64", "Config": {"Env": [IMAGE_ENV]}}),
        ),
        ("GET", ["volumes", name]) => {
            let removed = lock(state).removed_volumes.contains(&(*name).to_owned());
            volume(name).filter(|_| !removed)
        }
        ("DELETE", ["volumes", name]) => {
            lock(state).removed_volumes.push((*name).to_owned());
            volume(name).map(|_| Value::Null)
        }
        ("POST", ["containers", "create"]) => {
            let name = query.split('&').find_map(|pair| pair.strip_prefix("name="));
            let name = name.unwrap_or_default().to_owned();
            lock(state).helper = Some((name, serde_json::from_slice(&body)?));
            Some(json!({"Id": HELPER_ID, "Warnings": []}))
        }
        ("GET", ["containers", key, "json"]) => lock(state)
            .helper
            .as_ref()
            .filter(|(name, _)| [HELPER_ID, name.as_str()].contains(key))
            .map(|(name, create)| inspect(name, create)),
        ("DELETE", ["containers", HELPER_ID]) => lock(state).helper.take().map(|_| Value::Null),
        _ => None,
    };
    if killed_here {
        hung.notify_one();
        return std::future::pending().await;
    }
    let (status, body) = match response {
        Some(Value::Null) => ("204 No Content", String::new()),
        Some(body) => ("200 OK", body.to_string()),
        None => (
            "404 Not Found",
            r#"{"message":"No such object"}"#.to_owned(),
        ),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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
        let head_end = request.windows(4).position(|window| window == b"\r\n\r\n");
        if let Some(head_end) = head_end {
            let head = String::from_utf8_lossy(&request[..head_end]).into_owned();
            let length = head
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .map_or(Ok(0), |(_, value)| value.trim().parse::<usize>())?;
            if request.len() >= head_end + 4 + length {
                let mut words = head.split_whitespace().map(str::to_owned);
                let body = request[head_end + 4..head_end + 4 + length].to_vec();
                return Ok((
                    words.next().unwrap_or_default(),
                    words.next().unwrap_or_default(),
                    body,
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

fn volume(name: &str) -> Option<Value> {
    let role = match name.strip_prefix(PRIVATE_VOLUME)? {
        "-home" => "runner-home",
        "-work" => "runner-work",
        "-metadata" => "engine-metadata",
        _ => return None,
    };
    let labels = json!({"velnor.connection": CONNECTION_ID, "velnor.reservation": "2", "velnor.operation": "1", "velnor.role": role});
    Some(json!({
        "Name": name,
        "Driver": "local",
        "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"),
        "Labels": labels,
        "Scope": "local",
        "Options": {},
    }))
}

fn inspect(name: &str, create: &Value) -> Value {
    let mut config = create.clone();
    let host_config = config
        .as_object_mut()
        .and_then(|config| config.remove("HostConfig"))
        .unwrap_or_default();
    if config["Env"].is_null() {
        config["Env"] = json!([IMAGE_ENV]);
    }
    let mounts: Vec<Value> = host_config["Mounts"].as_array().into_iter().flatten().map(|mount| {
        json!({"Type": "volume", "Name": mount["Source"], "Destination": mount["Target"], "RW": !mount["ReadOnly"].as_bool().unwrap_or(false)})
    }).collect();
    json!({
        "Id": HELPER_ID,
        "Name": format!("/{name}"),
        "Image": config["Image"],
        "State": {"Status": "created", "Running": false, "StartedAt": "0001-01-01T00:00:00Z"},
        "Config": config,
        "HostConfig": host_config,
        "Mounts": mounts,
        "NetworkSettings": {"SandboxID": "", "SandboxKey": "", "Ports": {}, "Networks": {}},
    })
}

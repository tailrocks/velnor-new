// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;

use super::TestResult;
use super::seed::{
    CONNECTION_ID, ENGINE_ID, IMAGE_ID, OPERATION_ID, PRIVATE_VOLUME, RESERVATION_ID,
};

pub(super) const HELPER_ID: &str =
    "c0ffee0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
const IMAGE_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

struct Helper {
    name: String,
    create: Value,
}

#[derive(Default)]
struct State {
    helper: Option<Helper>,
    requests: Vec<String>,
}

pub(super) struct FixtureDocker {
    state: Arc<Mutex<State>>,
    created: Arc<Notify>,
}

impl FixtureDocker {
    pub(super) fn start(socket: &Path) -> TestResult<Self> {
        let listener = UnixListener::bind(socket)?;
        let state = Arc::new(Mutex::new(State::default()));
        let created = Arc::new(Notify::new());
        let (serve_state, serve_created) = (Arc::clone(&state), Arc::clone(&created));
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (state, created) = (Arc::clone(&serve_state), Arc::clone(&serve_created));
                tokio::spawn(async move { drop(serve(stream, &state, &created).await) });
            }
        });
        Ok(Self { state, created })
    }

    pub(super) async fn helper_created(&self) {
        self.created.notified().await;
    }

    pub(super) fn helper_present(&self) -> bool {
        lock(&self.state).helper.is_some()
    }

    pub(super) fn take_requests(&self) -> Vec<String> {
        std::mem::take(&mut lock(&self.state).requests)
    }
}

fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn serve(mut stream: UnixStream, state: &Mutex<State>, created: &Notify) -> TestResult<()> {
    let (method, path, body) = read_request(&mut stream).await?;
    let route = path
        .split('?')
        .next()
        .unwrap_or_default()
        .replace("%3A", ":");
    lock(state).requests.push(format!("{method} {route}"));
    let segments: Vec<&str> = route
        .trim_start_matches('/')
        .split('/')
        .skip_while(|segment| segment.starts_with("v1."))
        .collect();
    match (method.as_str(), segments.as_slice()) {
        ("GET", ["info"]) => respond(&mut stream, 200, &info()).await,
        ("GET", ["images", id, "json"]) if *id == IMAGE_ID => {
            respond(&mut stream, 200, &image()).await
        }
        ("GET", ["volumes", name]) => match volume(name) {
            Some(volume) => respond(&mut stream, 200, &volume).await,
            None => not_found(&mut stream).await,
        },
        ("POST", ["containers", "create"]) => {
            let name = query_value(&path, "name").unwrap_or_default();
            lock(state).helper = Some(Helper {
                name,
                create: serde_json::from_slice(&body)?,
            });
            created.notify_one();
            std::future::pending::<()>().await;
            Ok(())
        }
        ("GET", ["containers", key, "json"]) => {
            let inspected = lock(state)
                .helper
                .as_ref()
                .filter(|helper| *key == HELPER_ID || *key == helper.name)
                .map(inspect);
            match inspected {
                Some(inspected) => respond(&mut stream, 200, &inspected).await,
                None => not_found(&mut stream).await,
            }
        }
        ("DELETE", ["containers", key]) if *key == HELPER_ID => {
            lock(state).helper = None;
            respond_empty(&mut stream).await
        }
        _ => not_found(&mut stream).await,
    }
}

async fn read_request(stream: &mut UnixStream) -> TestResult<(String, String, Vec<u8>)> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    let head_end = loop {
        if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err("request closed early".into());
        }
        request.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&request[..head_end]).into_owned();
    let length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.trim().parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    while request.len() < head_end + length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err("request body closed early".into());
        }
        request.extend_from_slice(&chunk[..read]);
    }
    let mut words = head.split_whitespace();
    let method = words.next().unwrap_or_default().to_owned();
    let path = words.next().unwrap_or_default().to_owned();
    Ok((method, path, request[head_end..head_end + length].to_vec()))
}

fn query_value(path: &str, key: &str) -> Option<String> {
    path.split_once('?')?
        .1
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value.to_owned())
}

async fn respond(stream: &mut UnixStream, status: u16, body: &Value) -> TestResult<()> {
    let body = body.to_string();
    let head = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn respond_empty(stream: &mut UnixStream) -> TestResult<()> {
    stream
        .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
        .await?;
    stream.shutdown().await?;
    Ok(())
}

async fn not_found(stream: &mut UnixStream) -> TestResult<()> {
    respond(stream, 404, &json!({"message": "No such object"})).await
}

fn info() -> Value {
    json!({
        "ID": ENGINE_ID,
        "DockerRootDir": "/var/lib/docker",
        "OSType": "linux",
        "Architecture": "x86_64",
        "NCPU": 4,
        "MemTotal": 8_589_934_592_u64,
    })
}

fn image() -> Value {
    json!({
        "Id": IMAGE_ID,
        "Os": "linux",
        "Architecture": "amd64",
        "Config": {"Env": [IMAGE_ENV]},
    })
}

fn volume(name: &str) -> Option<Value> {
    let role = match name.strip_prefix(PRIVATE_VOLUME)? {
        "-home" => "runner-home",
        "-work" => "runner-work",
        "-metadata" => "engine-metadata",
        _ => return None,
    };
    Some(json!({
        "Name": name,
        "Driver": "local",
        "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"),
        "Labels": {
            "velnor.connection": CONNECTION_ID,
            "velnor.reservation": RESERVATION_ID.to_string(),
            "velnor.operation": OPERATION_ID.to_string(),
            "velnor.role": role,
        },
        "Scope": "local",
        "Options": {},
    }))
}

fn inspect(helper: &Helper) -> Value {
    let mut config = helper.create.clone();
    let host_config = config
        .as_object_mut()
        .and_then(|config| config.remove("HostConfig"))
        .unwrap_or(Value::Null);
    if config.get("Env").is_none_or(Value::is_null) {
        config["Env"] = json!([IMAGE_ENV]);
    }
    let mounts: Vec<Value> = host_config["Mounts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|mount| {
            json!({
                "Type": "volume",
                "Name": mount["Source"],
                "Source": format!("/var/lib/docker/volumes/{}/_data", mount["Source"].as_str().unwrap_or_default()),
                "Destination": mount["Target"],
                "Driver": "local",
                "Mode": "z",
                "RW": !mount["ReadOnly"].as_bool().unwrap_or(false),
                "Propagation": "",
            })
        })
        .collect();
    json!({
        "Id": HELPER_ID,
        "Name": format!("/{}", helper.name),
        "Image": config["Image"],
        "State": {"Status": "created", "Running": false, "StartedAt": "0001-01-01T00:00:00Z"},
        "Config": config,
        "HostConfig": host_config,
        "Mounts": mounts,
        "NetworkSettings": {"SandboxID": "", "SandboxKey": "", "Ports": {}, "Networks": {}},
    })
}

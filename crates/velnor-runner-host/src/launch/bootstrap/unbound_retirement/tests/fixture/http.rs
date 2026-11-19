// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

fn respond(state: &mut State, method: &str, segments: &[&str]) -> (u16, Value) {
    let found = |value: Option<Value>| {
        value.map_or((404, json!({"message": "No such object"})), |value| {
            (200, value)
        })
    };
    match (method, segments) {
        ("GET", ["info"]) => (200, json!({"ID": ENGINE_ID, "DockerRootDir": "/var/lib/docker", "OSType": "linux", "Architecture": "x86_64"})),
        ("GET", ["images", image, "json"]) => (200, json!({"Id": image, "Os": "linux", "Architecture": "amd64", "Config": {"Env": [initializer::PATH]}})),
        ("GET", ["containers", key, "json"]) => found(
            state
                .containers
                .iter()
                .find(|(id, container)| *id == key || container.name == *key)
                .map(|(id, container)| inspection(id, container)),
        ),
        ("POST", ["containers", id, "start"]) => match state.containers.get_mut(*id) {
            Some(container) => {
                container.running = true;
                container.fresh = false;
                (204, Value::Null)
            }
            None => found(None),
        },
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

pub(super) async fn serve(
    mut stream: UnixStream,
    state: &Mutex<State>,
    hung: &Notify,
) -> TestResult<()> {
    let (method, target, body) = read_request(&mut stream).await?;
    let (route, query) = target.split_once('?').unwrap_or((&target, ""));
    let route = unversioned(route);
    let request = format!("{method} {route}");
    let segments: Vec<&str> = route.split('/').skip(1).collect();
    let active = {
        let mut state = lock(state);
        if state.read_gate.as_deref() == Some(request.as_str()) {
            state.requests.push(request.clone());
            state.read_gate_hits += 1;
            false
        } else if state.armed {
            state.armed = false;
            state.hang_after = None;
            false
        } else {
            state.requests.push(request.clone());
            state.armed = state.hang_after.as_deref() == Some(request.as_str());
            true
        }
    };
    if !active {
        hung.notify_one();
        return std::future::pending().await;
    }
    if method == "POST" && segments.len() == 3 && segments[2] == "attach" {
        let ready = lock(state)
            .containers
            .get(segments[1])
            .and_then(|container| attach::ready(&container.name));
        return attach::serve(stream, ready).await;
    }
    let (status, body) = if method == "POST" && segments.len() == 2 && segments[1] == "create" {
        create(state, segments[0], query, &body).await?
    } else {
        let response = respond(&mut lock(state), &method, &segments);
        let gate = if method == "DELETE" && response.0 == 204 {
            lock(state).deletes.remove(&request)
        } else {
            None
        };
        if let Some(gate) = gate {
            gate.reached.notify_one();
            gate.release.notified().await;
        }
        response
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

fn unversioned(route: &str) -> &str {
    let Some((prefix, suffix)) = route
        .strip_prefix('/')
        .and_then(|path| path.split_once('/'))
    else {
        return route;
    };
    if prefix.strip_prefix('v').is_some_and(|version| {
        version.contains('.')
            && version
                .chars()
                .all(|value| value.is_ascii_digit() || value == '.')
    }) {
        &route[route.len() - suffix.len() - 1..]
    } else {
        route
    }
}

async fn create(
    state: &Mutex<State>,
    kind: &str,
    query: &str,
    body: &[u8],
) -> TestResult<(u16, Value)> {
    let body: Value = serde_json::from_slice(body)?;
    let name = if kind == "containers" {
        query
            .split('&')
            .find_map(|field| field.strip_prefix("name="))
            .ok_or("container name")?
    } else {
        body["Name"].as_str().ok_or("object name")?
    };
    let labels = body["Labels"].clone();
    if kind == "volumes" {
        lock(state).volumes.insert(name.to_owned(), labels.clone());
        return Ok((
            201,
            json!({"Name": name, "Driver": "local", "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"), "Labels": labels, "Scope": "local", "Options": {}}),
        ));
    }
    if kind == "containers" {
        let scripted = lock(state).scripted.remove(name);
        if let Some(pair) = scripted {
            lock(state).containers.insert(
                pair.helper_id(),
                Container {
                    name: name.to_owned(),
                    labels,
                    running: false,
                    fresh: true,
                    inspection: Some(initializer::inspection(pair)),
                },
            );
            return Ok((201, json!({"Id": pair.helper_id(), "Warnings": []})));
        }
    }
    let delayed = lock(state)
        .creates
        .remove(&(kind.to_owned(), name.to_owned()));
    let Some((id, gate)) = delayed else {
        return Ok((404, json!({"message": "No configured create"})));
    };
    *gate
        .submission
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(barriers::CreateSubmission {
        name: name.to_owned(),
        labels: labels.clone(),
    });
    gate.accepted.notify_one();
    gate.release.notified().await;
    match kind {
        "containers" => {
            lock(state).containers.insert(
                id.clone(),
                Container {
                    name: name.to_owned(),
                    labels,
                    running: false,
                    fresh: false,
                    inspection: None,
                },
            );
        }
        "networks" => {
            lock(state)
                .networks
                .insert(id.clone(), (name.to_owned(), labels));
        }
        _ => return Err("unsupported delayed create kind".into()),
    }
    gate.published.notify_one();
    Ok((201, json!({"Id": id, "Warnings": [], "Warning": ""})))
}

fn inspection(id: &str, container: &Container) -> Value {
    let mut value = container
        .inspection
        .clone()
        .unwrap_or_else(|| json!({"Config": {}}));
    value["Id"] = json!(id);
    value["Name"] = json!(format!("/{}", container.name));
    let status = match (container.running, container.fresh) {
        (true, _) => "running",
        (false, true) => "created",
        (false, false) => "exited",
    };
    let started_at = if container.fresh {
        "0001-01-01T00:00:00Z"
    } else {
        "2026-01-01T00:00:00Z"
    };
    value["State"] =
        json!({"Running": container.running, "Status": status, "StartedAt": started_at});
    value["Config"]["Labels"] = container.labels.clone();
    value
}

async fn read_request(stream: &mut UnixStream) -> TestResult<(String, String, Vec<u8>)> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(head_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&request[..head_end]).into_owned();
            let content_length = head
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                .map_or(Ok(0), |(_, value)| value.trim().parse::<usize>())?;
            let body_start = head_end + 4;
            while request.len() - body_start < content_length {
                let read = stream.read(&mut chunk).await?;
                if read == 0 {
                    return Err("the request body closed early".into());
                }
                request.extend_from_slice(&chunk[..read]);
            }
            let mut words = head.split_whitespace().map(str::to_owned);
            return Ok((
                words.next().unwrap_or_default(),
                words.next().unwrap_or_default(),
                request[body_start..body_start + content_length].to_vec(),
            ));
        }
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err("the request closed early".into());
        }
        request.extend_from_slice(&chunk[..read]);
    }
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Default)]
struct State {
    resources: HashMap<String, Value>,
    accepted: Option<String>,
    block_create: bool,
    block_inspect: bool,
    inspect_blocked: bool,
    fail_delete: bool,
    block_delete: bool,
    delete_started: bool,
    stop_new: bool,
    journal_path: Option<PathBuf>,
    delete_snapshot: Option<Value>,
}

fn reply(mut stream: &UnixStream, status: &str, value: Value) {
    let body = value.to_string();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    drop(stream.write_all(response.as_bytes()));
}

fn serve(mut stream: UnixStream, state: Arc<Mutex<State>>) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    let (header_end, length) = loop {
        let n = stream.read(&mut buffer).unwrap_or(0);
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..n]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|s| s.parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                break (end, length);
            }
        }
    };
    let request = String::from_utf8_lossy(&bytes[..header_end]);
    let first = request.lines().next().unwrap_or("");
    let mut fields = first.split_whitespace();
    let method = fields.next().unwrap_or("");
    let path = fields.next().unwrap_or("");
    let path = path.strip_prefix("/v1.53").unwrap_or(path);
    if path == "/version" {
        return reply(&stream, "200 OK", json!({"Version":"1.0"}));
    }
    if path == "/info" {
        return reply(
            &stream,
            "200 OK",
            json!({"ID":"daemon-id", "DockerRootDir":"/var/lib/docker"}),
        );
    }
    if path.starts_with("/images/") {
        if state.lock().unwrap().stop_new {
            return reply(
                &stream,
                "500 Internal Server Error",
                json!({"message":"stop"}),
            );
        }
        let dind = path.contains("dddddddd");
        let id = if dind {
            format!("sha256:{}", "d".repeat(64))
        } else {
            format!("sha256:{}", "c".repeat(64))
        };
        return reply(
            &stream,
            "200 OK",
            json!({"Id":id,"Os":"linux","Architecture":"amd64","Config":{"User":if dind {""} else {"runner"},"Entrypoint":[if dind {"/usr/local/bin/velnor-dind-entrypoint"} else {"/usr/local/bin/velnor-runner-entrypoint"}],"Labels":{"org.opencontainers.image.revision":SHA,"io.velnor.image.kind":if dind {"dind"} else {"runner"}}}}),
        );
    }
    if method == "POST" && path.starts_with("/containers/create?") {
        let name = path
            .split("name=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .to_string();
        let body: Value =
            serde_json::from_slice(&bytes[header_end + 4..header_end + 4 + length]).unwrap();
        let mut locked = state.lock().unwrap();
        locked.resources.insert(
            name.clone(),
            json!({"Id":ID,"Name":format!("/{name}"),"Config":{"Labels":body["Labels"]}}),
        );
        locked.accepted = Some(name);
        let block = locked.block_create;
        drop(locked);
        if block {
            let end = Instant::now() + Duration::from_secs(30);
            while state.lock().unwrap().block_create && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        return reply(&stream, "201 Created", json!({"Id":ID,"Warnings":[]}));
    }
    if path.starts_with("/volumes/") {
        let name = path
            .trim_start_matches("/volumes/")
            .split('?')
            .next()
            .unwrap();
        let mut locked = state.lock().unwrap();
        if method == "GET" {
            return match locked.resources.get(name) {
                Some(volume) => reply(&stream, "200 OK", volume.clone()),
                None => reply(&stream, "404 Not Found", json!({"message":"missing"})),
            };
        }
        if method == "DELETE" {
            if let Some(path) = &locked.journal_path {
                locked.delete_snapshot =
                    Some(serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap());
            }
            locked.delete_started = true;
            let block = locked.block_delete;
            drop(locked);
            if block {
                let end = Instant::now() + Duration::from_secs(30);
                while state.lock().unwrap().block_delete && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            state.lock().unwrap().resources.remove(name);
            return reply(&stream, "204 No Content", json!({}));
        }
    }
    if path.starts_with("/containers/") {
        let name = path
            .trim_start_matches("/containers/")
            .split('/')
            .next()
            .unwrap()
            .split('?')
            .next()
            .unwrap();
        let mut locked = state.lock().unwrap();
        let key = locked
            .resources
            .iter()
            .find(|(key, value)| key.as_str() == name || value["Id"] == name)
            .map(|(key, _)| key.clone());
        if method == "GET" {
            if locked.block_inspect {
                locked.inspect_blocked = true;
                drop(locked);
                std::thread::sleep(Duration::from_secs(30));
                return;
            }
            return match key {
                Some(key) => reply(&stream, "200 OK", locked.resources[&key].clone()),
                None => reply(&stream, "404 Not Found", json!({"message":"missing"})),
            };
        }
        if method == "DELETE" {
            if let Some(path) = &locked.journal_path {
                locked.delete_snapshot =
                    Some(serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap());
            }
            if locked.fail_delete {
                return reply(&stream, "403 Forbidden", json!({"message":"failed"}));
            }
            if let Some(key) = key {
                locked.delete_started = true;
                let block = locked.block_delete;
                drop(locked);
                if block {
                    let end = Instant::now() + Duration::from_secs(30);
                    while state.lock().unwrap().block_delete && Instant::now() < end {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                state.lock().unwrap().resources.remove(&key);
                return reply(&stream, "204 No Content", json!({}));
            }
            return reply(&stream, "404 Not Found", json!({"message":"missing"}));
        }
    }
    reply(
        &stream,
        "500 Internal Server Error",
        json!({"message":format!("unexpected {method} {path}")}),
    );
}

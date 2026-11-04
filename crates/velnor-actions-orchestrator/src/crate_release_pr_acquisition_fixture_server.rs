// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Routes, Server, WriterExpectation};
use serde_json::Value;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

pub(super) fn server(
    routes: Routes,
    expected: WriterExpectation,
) -> Result<Server, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}", listener.local_addr()?);
    let stop = Arc::new(AtomicBool::new(false));
    let running = Arc::clone(&stop);
    let log = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::clone(&log);
    let thread = thread::spawn(move || {
        let mut branch = false;
        let mut pull = false;

        while !running.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).expect("blocking stream");
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .expect("timeout");
                    let bytes = read_request(&mut stream);
                    let request = String::from_utf8_lossy(&bytes);
                    let route = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("");
                    let method = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().next())
                        .unwrap_or("");
                    let body = request.split_once("\r\n\r\n").map_or("", |(_, body)| body);
                    let payload = if body.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_str(body).expect("JSON payload")
                    };
                    events
                        .lock()
                        .expect("events")
                        .push((format!("{method} {route}"), payload.clone()));
                    let value = reply(method, route, &routes, &expected, &mut branch, &mut pull);
                    let status = if value.is_some() {
                        if method == "POST" {
                            "201 Created"
                        } else {
                            "200 OK"
                        }
                    } else {
                        "404 Not Found"
                    };
                    let body = value.unwrap_or_else(|| serde_json::json!({})).to_string();
                    write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("reply");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    });
    Ok((url, stop, thread, log))
}

fn read_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let len = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .and_then(|n| n.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + len {
                break;
            }
        }
        let Ok(count) = stream.read(&mut buffer) else {
            break;
        };
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    bytes
}

fn reply(
    method: &str,
    route: &str,
    routes: &Routes,
    expected: &WriterExpectation,
    branch: &mut bool,
    pull: &mut bool,
) -> Option<Value> {
    let head = "d".repeat(40);
    let branch_name = "release-plz-42-1";
    let title = "chore: release";
    let description = "Automated release preparation. Review the version, dependency and changelog changes before merging.";
    if method == "GET" && route.contains("/git/matching-refs/heads/") {
        Some(if *branch {
            serde_json::json!([{"ref":format!("refs/heads/{branch_name}"),"object":{"type":"commit","sha":head}}])
        } else {
            serde_json::json!([])
        })
    } else if method == "GET" && route.contains("/commits/") && route.contains("/pulls?") {
        Some(serde_json::json!([]))
    } else if method == "GET" && route.ends_with(&format!("/git/commits/{head}")) {
        let email = "41898282+github-actions[bot]@users.noreply.github.com";
        Some(
            serde_json::json!({"sha":head,"tree":{"sha":expected.root},"parents":[{"sha":expected.source}],"message":title,"author":{"name":"github-actions[bot]","email":email},"committer":{"name":"github-actions[bot]","email":email}}),
        )
    } else if method == "GET" && route.contains("/pulls?state=open&") {
        Some(serde_json::json!([]))
    } else if method == "GET" && route.contains("/pulls?") {
        Some(if *pull {
            serde_json::json!([{"number":17,"base":{"repo":{"full_name":expected.repo},"ref":expected.base},"head":{"repo":{"full_name":expected.repo},"ref":branch_name,"sha":head},"user":{"id":41_898_282},"state":"open","title":title,"body":description}])
        } else {
            serde_json::json!([])
        })
    } else if method == "POST" && route.ends_with("/git/blobs") {
        Some(serde_json::json!({"sha":expected.blob}))
    } else if method == "POST" && route.ends_with("/git/trees") {
        Some(serde_json::json!({"sha":expected.root}))
    } else if method == "POST" && route.ends_with("/git/commits") {
        Some(serde_json::json!({"sha":head}))
    } else if method == "POST" && route.ends_with("/git/refs") {
        *branch = true;
        Some(serde_json::json!({}))
    } else if method == "POST" && route.ends_with("/pulls") {
        *pull = true;
        Some(serde_json::json!({}))
    } else if method == "GET" {
        routes.lock().expect("routes").get(route).cloned()
    } else {
        None
    }
}

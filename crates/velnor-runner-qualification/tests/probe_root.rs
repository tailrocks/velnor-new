// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![expect(
    clippy::unwrap_used,
    reason = "the external Docker fixture fails immediately on invalid test state"
)]

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use velnor_runner_qualification::{QualificationError, open_qualification_session, qualify_probe};

fn fixture(name: &str) -> (std::path::PathBuf, Arc<Mutex<Vec<String>>>) {
    let directory = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("probe-root-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let socket = directory.join("d");
    drop(std::fs::remove_file(&socket));
    let listener = UnixListener::bind(&socket).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let seen = Arc::clone(&seen);
            std::thread::spawn(move || {
                let mut buffer = [0; 8192];
                let n = stream.read(&mut buffer).unwrap_or(0);
                let text = String::from_utf8_lossy(&buffer[..n]).into_owned();
                seen.lock().unwrap().push(text);
                let body = r#"{"message":"refused"}"#;
                drop(stream.write_all(
                    format!(
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                ));
            });
        }
    });
    (socket, requests)
}

#[tokio::test]
async fn probe_refuses_noncanonical_or_root_docker_directory_before_any_docker_request() {
    let (socket, requests) = fixture("probe");
    let docker = bollard::Docker::connect_with_socket(
        socket.to_str().unwrap(),
        30,
        bollard::API_DEFAULT_VERSION,
    )
    .unwrap();
    for root in [
        "/",
        "/var/lib/docker/../../etc",
        "/var/lib/docker/./lib",
        "//var/lib/docker",
        "/var/lib/docker/",
        "var/lib/docker",
    ] {
        let result = qualify_probe(
            &docker,
            &format!("sha256:{}", "e".repeat(64)),
            root,
            None,
            Instant::now() + Duration::from_secs(10),
        )
        .await;
        assert_eq!(result, Err(QualificationError::Identity), "root {root}");
        let session = open_qualification_session(
            docker.clone(),
            socket.with_file_name("journal.json"),
            root.into(),
        )
        .await;
        assert_eq!(session.err(), Some(QualificationError::Identity), "{root}");
    }
    let seen = requests.lock().unwrap();
    assert!(
        seen.iter()
            .all(|request| !request.contains("/containers/create")),
        "{seen:?}"
    );
    assert!(seen.is_empty(), "{seen:?}");
    drop(std::fs::remove_dir_all(socket.parent().unwrap()));
}

#[tokio::test]
async fn probe_accepts_canonical_docker_directory_past_the_root_guard() {
    let (socket, requests) = fixture("accept");
    let docker = bollard::Docker::connect_with_socket(
        socket.to_str().unwrap(),
        30,
        bollard::API_DEFAULT_VERSION,
    )
    .unwrap();
    let result = qualify_probe(
        &docker,
        &format!("sha256:{}", "e".repeat(64)),
        "/var/lib/docker",
        None,
        Instant::now() + Duration::from_secs(10),
    )
    .await;
    assert_ne!(result, Err(QualificationError::Identity));
    assert!(!requests.lock().unwrap().is_empty());
    drop(std::fs::remove_dir_all(socket.parent().unwrap()));
}

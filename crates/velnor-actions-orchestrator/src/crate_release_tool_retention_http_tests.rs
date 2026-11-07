// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[test]
fn release_retention_bounds_raw_whitespace_across_pages_and_passes()
-> Result<(), Box<dyn std::error::Error>> {
    let sha = "a".repeat(40);
    let request = GitHubAdmissionRequest {
        repository: "owner/project".into(),
        default_branch: "main".into(),
        environment: "release".into(),
        trusted_reviewer_id: 1,
        workflow_path: ".github/workflows/release.yml".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        workflow_sha: sha.clone(),
        run_id: 12,
        run_attempt: 1,
        event: "push".into(),
        source_sha: sha.clone(),
        recovery_sha: None,
    };
    let make = |id, purpose| json!({"id":id,"name":format!("velnor-release-{purpose}-12-1"),"digest":format!("sha256:{}","c".repeat(64)),"size_in_bytes":100,"expired":false,"created_at":"2026-01-01T00:00:10Z","expires_at":"2026-02-05T00:00:00Z","workflow_run":{"id":12,"repository_id":8,"head_repository_id":8,"head_sha":sha}});
    let receipt = make(10, "receipt");
    let rust = make(11, "rust");
    let mut entries: Vec<Value> = (0..201)
        .map(|i| json!({"id":1000+i,"name":format!("other-{i}")}))
        .collect();
    entries[0] = receipt.clone();
    entries[200] = rust.clone();
    let mut routes = BTreeMap::from([
        (
            "/repos/owner/project".to_owned(),
            json!({"id":8,"full_name":"owner/project"}),
        ),
        (
            "/repos/owner/project/actions/runs/12".to_owned(),
            json!({"id":12,"run_attempt":1,"head_sha":sha,"repository":{"id":8},"head_repository":{"id":8},"created_at":"2026-01-01T00:00:00Z","run_started_at":"2026-01-01T00:00:00Z"}),
        ),
        (
            "/repos/owner/project/actions/artifacts/10".to_owned(),
            receipt,
        ),
        ("/repos/owner/project/actions/artifacts/11".to_owned(), rust),
    ]);
    for (page, chunk) in entries.chunks(100).enumerate() {
        let base = "/repos/owner/project/actions/runs/12/artifacts?per_page=100";
        let path = if page == 0 {
            base.to_owned()
        } else {
            format!("{base}&page={}", page + 1)
        };
        routes.insert(path, json!({"total_count":201,"artifacts":chunk}));
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let origin = format!("http://{}", listener.local_addr()?);
    let stop = Arc::new(AtomicBool::new(false));
    let running = Arc::clone(&stop);
    let page_count = Arc::new(AtomicUsize::new(0));
    let pages = Arc::clone(&page_count);
    let server = serve(listener, routes, running, pages);
    let github = crate::crate_release_github::NativeGitHubAdmissionRead {
        reader: crate::crate_release_http::GitHubAdmissionHttp::test_client(&origin)?,
    };
    let result = verify_preflight_retention(
        &request,
        &github,
        10,
        11,
        timestamp("2026-01-01T01:00:00Z").ok_or("now")?,
    );
    stop.store(true, Ordering::Relaxed);
    server.join().map_err(|_| "server")?;
    assert!(
        matches!(result, Err(OrchestratorError::Internal { ref problem }) if problem == "crate_release_http_response"),
        "unexpected result: {result:?}"
    );
    assert!(
        page_count.load(Ordering::Relaxed) >= 5,
        "did not reach the shared budget across both passes"
    );
    Ok(())
}

fn serve(
    listener: TcpListener,
    routes: BTreeMap<String, Value>,
    running: Arc<AtomicBool>,
    pages: Arc<AtomicUsize>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let padding = vec![b' '; 7 * 1024 * 1024];
        let deadline = Instant::now() + Duration::from_secs(60);
        while !running.load(Ordering::Relaxed) && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    if stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .is_err()
                    {
                        continue;
                    }
                    let mut bytes = vec![0; 4096];
                    let Ok(count) = stream.read(&mut bytes) else {
                        continue;
                    };
                    let raw = String::from_utf8_lossy(&bytes[..count]);
                    let path = raw
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("");
                    let Some(value) = routes.get(path) else {
                        drop(stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"));
                        continue;
                    };
                    let Ok(body) = serde_json::to_vec(value) else {
                        continue;
                    };
                    let pad = if path.contains("/artifacts?") {
                        pages.fetch_add(1, Ordering::Relaxed);
                        padding.as_slice()
                    } else {
                        &[]
                    };
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        pad.len() + body.len()
                    );
                    drop(
                        stream
                            .write_all(header.as_bytes())
                            .and_then(|()| stream.write_all(pad))
                            .and_then(|()| stream.write_all(&body)),
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(_) => break,
            }
        }
    })
}

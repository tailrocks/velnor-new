// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::net::{SocketAddr, TcpListener};
use std::process::Stdio;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::Request;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

use super::{CACHES, LIMIT, archives};
use crate::support::{Harness, MAIN, PULL_2, Reply, temp_dir};

const TOKEN: &str = "Bearer local-test-control";

struct Listening {
    child: Child,
    job: SocketAddr,
    control: Option<SocketAddr>,
    stderr: tokio::task::JoinHandle<()>,
}

impl Listening {
    async fn start(harness: &Harness, name: &str, control: bool) -> Self {
        let job = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
        let admin = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
        let job_address = job.local_addr().unwrap_or_else(|e| unreachable!("{e}"));
        let admin_address = admin.local_addr().unwrap_or_else(|e| unreachable!("{e}"));
        let dir = temp_dir(name);
        std::fs::write(dir.join("pulls.json"), "[]").unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(dir.join("control-token"), "local-test-control\n")
            .unwrap_or_else(|e| unreachable!("{e}"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-local"));
        command
            .arg("api-stub")
            .arg("--listen")
            .arg(job_address.to_string())
            .arg("--repo")
            .arg(&dir)
            .arg("--dir")
            .arg(&dir)
            .arg("--cache-dir")
            .arg(harness.dir())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if control {
            command
                .arg("--control-listen")
                .arg(admin_address.to_string())
                .arg("--control-token")
                .arg(dir.join("control-token"));
        }
        drop((job, admin));
        let mut child = command.spawn().unwrap_or_else(|e| unreachable!("{e}"));
        let stderr = child
            .stderr
            .take()
            .unwrap_or_else(|| unreachable!("stderr is piped"));
        let mut lines = BufReader::new(stderr).lines();
        let ready = async {
            let mut listening = 0;
            while listening < if control { 2 } else { 1 } {
                let line = lines
                    .next_line()
                    .await
                    .unwrap_or_else(|e| unreachable!("{e}"))
                    .unwrap_or_else(|| unreachable!("api-stub stopped before listening"));
                assert!(line.contains("listening on"), "{line}");
                listening += 1;
            }
        };
        tokio::time::timeout(Duration::from_secs(10), ready)
            .await
            .unwrap_or_else(|e| unreachable!("api-stub did not start: {e}"));
        let stderr = tokio::spawn(async move {
            while lines.next_line().await.is_ok_and(|line| line.is_some()) {}
        });
        Self {
            child,
            job: job_address,
            control: control.then_some(admin_address),
            stderr,
        }
    }

    async fn stop(mut self) {
        self.child
            .kill()
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
        self.stderr.await.unwrap_or_else(|e| unreachable!("{e}"));
    }
}

async fn call(address: SocketAddr, method: &str, path: &str, bearer: Option<&str>) -> Reply {
    let stream = TcpStream::connect(address)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    tokio::spawn(async move {
        connection.await.unwrap_or_else(|e| unreachable!("{e}"));
    });
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", address.to_string());
    if let Some(bearer) = bearer {
        request = request.header("authorization", bearer);
    }
    let response = sender
        .send_request(
            request
                .body(Empty::<Bytes>::new())
                .unwrap_or_else(|e| unreachable!("{e}")),
        )
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .to_bytes();
    Reply {
        status,
        headers,
        body,
    }
}

async fn preserved(harness: &Harness, before: &[std::path::PathBuf]) {
    assert_eq!(archives(harness), before);
    for (git_ref, key, bytes) in [(MAIN, "tools", b"1234"), (PULL_2, "pr-tools", b"5678")] {
        let restored = harness.restore(git_ref, key).await;
        assert_eq!(restored["ok"], true);
        let url = restored["signed_download_url"].as_str().unwrap_or_default();
        assert_eq!(harness.get(url).await.body.as_ref(), bytes);
    }
}

async fn boundary(name: &str, control: bool) {
    let harness = Harness::new(name, LIMIT).await;
    harness.register(2).await;
    let saved = harness.save(MAIN, "tools", b"1234").await;
    let pr = harness.save(PULL_2, "pr-tools", b"5678").await;
    let id = saved["entry_id"].as_str().unwrap_or_default();
    let pr_id = pr["entry_id"].as_str().unwrap_or_default();
    let server = Listening::start(&harness, &format!("{name}-server"), control).await;
    let before = archives(&harness);
    for bearer in [None, Some("Bearer wrong"), Some(TOKEN)] {
        for prefix in ["", "/"] {
            let list = format!("{prefix}{CACHES}?per_page=100");
            assert_eq!(call(server.job, "GET", &list, bearer).await.status, 403);
            for id in [id, pr_id] {
                let delete = format!("{prefix}{CACHES}/{id}");
                assert_eq!(
                    call(server.job, "DELETE", &delete, bearer).await.status,
                    403
                );
            }
        }
    }
    preserved(&harness, &before).await;
    assert_eq!(
        call(server.job, "GET", "/repos/o/r/pulls", None)
            .await
            .status,
        200
    );
    if let Some(address) = server.control {
        for bearer in [None, Some("Bearer wrong")] {
            assert_eq!(call(address, "GET", CACHES, bearer).await.status, 401);
            assert_eq!(
                call(address, "DELETE", &format!("{CACHES}/{id}"), bearer)
                    .await
                    .status,
                401
            );
        }
        preserved(&harness, &before).await;
        let list = call(address, "GET", CACHES, Some(TOKEN)).await;
        assert_eq!(list.status, 200);
        assert_eq!(list.json()["total_count"], 2);
        assert_eq!(
            call(address, "DELETE", &format!("{CACHES}/{id}"), Some(TOKEN))
                .await
                .status,
            204
        );
        assert_eq!(harness.restore(MAIN, "tools").await["ok"], false);
        assert_eq!(harness.restore(PULL_2, "pr-tools").await["ok"], true);
        assert_eq!(archives(&harness).len(), 1);
    }
    server.stop().await;
}

#[tokio::test]
async fn cache_management_is_authenticated_and_available_only_on_the_control_listener() {
    boundary("api-listener-control", true).await;
}

#[tokio::test]
async fn cache_management_stays_disabled_without_a_control_listener() {
    boundary("api-listener-no-control", false).await;
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use serde_json::json;
use tokio::net::TcpListener;
use velnor_local::cli::{Cli, Command, RunArgs};
use velnor_local::io::drive::{Event, dispatch};
use velnor_local::io::env::{Engine, Environment};

use crate::support::temp_dir;

const REPLACED_AFTER: usize = 5;
const CONSUMED: &str = "env-jit-a1-1";
const FRESH: &str = "env-jit-b2-2";

#[derive(Debug, Default)]
struct RunnerServer {
    listed: AtomicUsize,
    client_started: Mutex<Vec<bool>>,
}

impl RunnerServer {
    fn replaced(&self) -> bool {
        self.listed.load(Ordering::SeqCst) >= REPLACED_AFTER
    }

    fn answer(&self, method: &str, uri: &str) -> String {
        match (method, uri) {
            ("GET", "/_apis/v1/Agent/0") => {
                let mut agents = vec![json!({"id": 1, "name": CONSUMED})];
                if self.listed.fetch_add(1, Ordering::SeqCst) >= REPLACED_AFTER {
                    agents.push(json!({"id": 2, "name": FRESH}));
                }
                json!({"value": agents, "count": agents.len()}).to_string()
            }
            ("GET", uri) if uri.starts_with("/_apis/v1/Message/isagentonline?name=") => {
                let online = uri.ends_with(CONSUMED) || self.replaced();
                json!({"online": online}).to_string()
            }
            ("GET", "/_apis/v1/Message/workflow/runs") => "[]".to_owned(),
            _ => json!({"id": "1-push", "side": "trusted", "event": "push", "existing": 0})
                .to_string(),
        }
    }
}

async fn serve(state: &'static RunnerServer) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"));
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let service = service_fn(move |request: Request<Incoming>| async move {
                let body = state.answer(request.method().as_str(), &request.uri().to_string());
                Ok::<_, io::Error>(Response::new(Full::new(Bytes::from(body)).boxed()))
            });
            tokio::spawn(http1::Builder::new().serve_connection(TokioIo::new(stream), service));
        }
    });
    address.to_string()
}

#[derive(Clone, Copy, Debug)]
struct Docker {
    server: &'static RunnerServer,
    address: &'static str,
}

impl Engine for Docker {
    fn docker(&self, args: &[&str]) -> impl Future<Output = io::Result<String>> + Send {
        let answer = match args {
            ["port", container, "81/tcp"] if container.ends_with("-cache") => {
                Ok(self.address.to_owned())
            }
            ["port", ..] => Err(io::Error::other("no public port")),
            _ => Ok(String::new()),
        };
        std::future::ready(answer)
    }

    fn logged(&self, args: &[&str], _: &Path) -> impl Future<Output = io::Result<bool>> + Send {
        if args.first() == Some(&"run") {
            let mut started = self
                .server
                .client_started
                .lock()
                .unwrap_or_else(|e| unreachable!("{e}"));
            started.push(self.server.replaced());
        }
        std::future::ready(Ok(true))
    }
}

fn run_args(dir: &Path) -> RunArgs {
    let dir = dir.display().to_string();
    let parsed = Cli::try_parse_from([
        "velnor-local",
        "run",
        "warm",
        "--dir",
        &dir,
        "--repo",
        &dir,
        "--runners",
        "1",
    ]);
    let Ok(Cli {
        command: Command::Run(run),
    }) = parsed
    else {
        unreachable!("{parsed:?}")
    };
    run
}

async fn send_after_a_consumed_pair(decisions: &str) -> Vec<bool> {
    let server: &'static RunnerServer = Box::leak(Box::default());
    let address: &'static str = Box::leak(serve(server).await.into_boxed_str());
    let dir = temp_dir("dispatch-replacement");
    for folder in ["keys", "events", "logs", "runner", "repo"] {
        std::fs::create_dir_all(dir.join(folder)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    std::fs::write(dir.join("keys/control"), "token").unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("runner/decisions.jsonl"), decisions)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let env = Environment::at(&dir, Docker { server, address })
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let payload = json!({"ref": "refs/heads/main"});
    let event = Event {
        name: "push",
        payload: &payload,
        git_ref: "refs/heads/main",
        sha: "0123456789abcdef0123456789abcdef01234567",
    };
    let sent = dispatch(&run_args(&dir), &env, (address, "token"), &event).await;
    assert!(sent.is_ok(), "{sent:?}");
    let started = server
        .client_started
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .clone();
    std::fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    started
}

#[tokio::test]
async fn the_driver_holds_the_next_event_until_the_replacement_pair_is_online() {
    let served =
        format!(r#"{{"pair":"trusted-1","side":"trusted","agent":"{CONSUMED}","allow":true}}"#);
    let started = send_after_a_consumed_pair(&served).await;
    assert_eq!(
        started,
        [true],
        "Runner.Client was sent before the replacement pair was online, while the consumed agent still answered online"
    );
}

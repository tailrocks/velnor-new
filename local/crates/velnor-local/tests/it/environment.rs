// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use velnor_local::cli::{EnvironmentArgs, RunnerMode};
use velnor_local::event::{PullRequest, Push, Repository};
use velnor_local::io::env::Environment;
use velnor_local::io::{guarded, open};

use crate::fake_docker::{FakeDocker, after};
use crate::support::temp_dir;

fn plain(dir: &std::path::Path, runners: u16, fork_runners: u16) -> EnvironmentArgs {
    EnvironmentArgs {
        dir: dir.to_path_buf(),
        runners,
        fork_runners,
        default_branch: "main".to_owned(),
        runner: RunnerMode::Plain,
        velnor_host: Some("velnor-host".into()),
        policy: None,
        label: Vec::new(),
    }
}

#[tokio::test]
async fn an_environment_that_fails_to_start_leaves_no_container_or_network() {
    let dir = temp_dir("environment-fails");
    let docker = FakeDocker::default();
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let args = plain(&dir, 2, 1);
    let started = env.up(&dir, &args).await;
    assert!(started.is_err());
    assert_eq!(docker.left(), Vec::<String>::new());
}

fn request(authorization: Option<&str>) -> hyper::Request<velnor_local::io::Body> {
    use http_body_util::BodyExt;
    let mut builder = hyper::Request::get("/repos/o/r/pulls");
    if let Some(value) = authorization {
        builder = builder.header("authorization", value);
    }
    builder
        .body(
            http_body_util::Empty::new()
                .map_err(|never| match never {})
                .boxed_unsync(),
        )
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[tokio::test]
async fn the_control_port_answers_only_requests_that_carry_the_control_token() {
    let handled = |_| async {
        use http_body_util::BodyExt;
        let body: velnor_local::io::Body = http_body_util::Empty::new()
            .map_err(|never| match never {})
            .boxed_unsync();
        let mut response = hyper::Response::new(body);
        *response.status_mut() = hyper::StatusCode::NO_CONTENT;
        response
    };
    let token = "0f1e2d";
    for (authorization, status) in [
        (Some("Bearer 0f1e2d"), 204),
        (Some("Bearer 0f1e2e"), 401),
        (Some("0f1e2d"), 401),
        (None, 401),
    ] {
        let response = guarded(token, request(authorization), handled).await;
        assert_eq!(response.status().as_u16(), status, "{authorization:?}");
    }
}

#[tokio::test]
async fn the_untrusted_side_has_its_own_network_and_keys_and_no_way_to_the_stub() {
    let dir = temp_dir("environment-sides");
    let docker = FakeDocker::default();
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let args = plain(&dir, 1, 1);
    assert!(env.up(&dir, &args).await.is_err());
    let (trusted, untrusted) = (env.name.clone(), format!("{}-fork", env.name));
    let started = docker.started();
    let names: Vec<&str> = started
        .iter()
        .flat_map(|call| after(call, "--name"))
        .collect();
    for role in [
        "standin",
        "cache",
        "api",
        "runner-1",
        "fork-standin",
        "fork-cache",
        "fork-runner-1",
    ] {
        assert!(
            names.contains(&format!("{trusted}-{role}").as_str()),
            "{role}"
        );
    }
    let keys = env.dir.join("keys").display().to_string();
    for call in &started {
        let name = after(call, "--name")[0];
        let mounts = after(call, "-v");
        let untrusted_role = name.starts_with(&format!("{trusted}-fork-"));
        let network = if untrusted_role { &untrusted } else { &trusted };
        assert_eq!(after(call, "--network"), [network.as_str()], "{name}");
        let restoring = call.iter().any(|arg| arg == "--restore-only");
        assert_eq!(restoring, name.ends_with("-fork-cache"), "{name}");
        for published in after(call, "-p") {
            assert_eq!(published, "127.0.0.1::81", "{name}");
        }
        if untrusted_role {
            assert!(
                !after(call, "--network-alias").contains(&"api-stub"),
                "{name}"
            );
            assert!(
                mounts
                    .iter()
                    .all(|mount| !mount.contains(":/api") && !mount.contains(":/repo")),
                "{name}"
            );
            assert!(
                mounts
                    .iter()
                    .filter(|mount| mount.contains("/token."))
                    .all(|mount| mount.starts_with(&format!("{keys}/fork/"))),
                "{name}"
            );
        } else {
            assert!(
                mounts.iter().all(|mount| !mount.contains("/keys/fork")),
                "{name}"
            );
        }
    }
    let mode = std::fs::metadata(dir.join("keys/control")).map_or_else(
        |e| unreachable!("{e}"),
        |meta| meta.permissions().mode() & 0o777,
    );
    assert_eq!(mode, 0o600);
    let keys_mode = std::fs::metadata(dir.join("keys")).map_or_else(
        |e| unreachable!("{e}"),
        |meta| meta.permissions().mode() & 0o777,
    );
    assert_eq!(keys_mode, 0o700);
}

#[tokio::test]
async fn an_event_runs_on_the_side_its_payload_belongs_to() {
    let dir = temp_dir("environment-client");
    let docker = FakeDocker::default();
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let repository = Repository {
        full_name: "tailrocks/velnor-lab",
        default_branch: "main",
        id: None,
    };
    let sha = "007c9fa8aac37d2aa7bc4a26876bf2654699f6bc";
    let pull = |fork| {
        repository.pull_request(&PullRequest {
            action: "opened",
            number: 1,
            base: ("main", sha),
            head: ("patch", sha),
            before: None,
            merged: false,
            merge_commit: sha,
            fork,
            closed_at: None,
            changed_files: 1,
        })
    };
    let push = repository.push(&Push {
        branch: "main",
        before: sha,
        after: sha,
        commits: &[],
    });
    let fork = format!("{}-fork", env.name);
    for (payload, network) in [
        (pull(true), fork.as_str()),
        (pull(false), env.name.as_str()),
        (push, env.name.as_str()),
    ] {
        let sent = env
            .client(&payload, &["image"], &dir.join("client.log"))
            .await;
        assert!(sent.is_ok_and(|ok| ok));
        let calls = docker.calls.lock().unwrap_or_else(|e| unreachable!("{e}"));
        let last = calls.last().cloned().unwrap_or_default();
        assert_eq!(after(&last, "--network"), [network]);
    }
}

#[tokio::test]
async fn the_open_port_refuses_pull_request_registration() {
    use http_body_util::BodyExt;
    let handled = |_| async {
        let body: velnor_local::io::Body = http_body_util::Empty::new()
            .map_err(|never| match never {})
            .boxed_unsync();
        let mut response = hyper::Response::new(body);
        *response.status_mut() = hyper::StatusCode::NO_CONTENT;
        response
    };
    for (method, path, status) in [
        ("PUT", "/_velnor/pull-requests/3", 403),
        ("GET", "/_velnor/pull-requests/3", 403),
        (
            "POST",
            "/twirp/github.actions.results.api.v1.CacheService/GetCacheEntryDownloadURL",
            204,
        ),
        ("GET", "/repos/o/r/pulls", 204),
    ] {
        let request = hyper::Request::builder()
            .method(method)
            .uri(path)
            .body(
                http_body_util::Empty::new()
                    .map_err(|never| match never {})
                    .boxed_unsync(),
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
        let response = open(request, handled).await;
        assert_eq!(response.status().as_u16(), status, "{method} {path}");
    }
}

#[tokio::test]
async fn a_malformed_control_token_is_replaced_by_a_full_one() {
    let dir = temp_dir("environment-token");
    std::fs::create_dir_all(dir.join("keys")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("keys/control"), "0f1e").unwrap_or_else(|e| unreachable!("{e}"));
    let docker = FakeDocker::default();
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let args = plain(&dir, 1, 0);
    assert!(env.up(&dir, &args).await.is_err());
    let token = std::fs::read_to_string(dir.join("keys/control")).unwrap_or_default();
    assert_eq!(token.len(), 64, "{token}");
    assert!(
        token.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{token}"
    );
    assert!(!dir.join("keys/control.new").exists());
}

#[test]
fn a_control_token_without_a_control_listener_is_refused() {
    use clap::Parser;
    let parse = |extra: &[&str]| {
        let base = [
            "velnor-local",
            "cache-server",
            "--upstream",
            "u:80",
            "--dir",
            "d",
            "--token-key",
            "k",
        ];
        velnor_local::cli::Cli::try_parse_from(base.iter().chain(extra)).is_ok()
    };
    assert!(parse(&[]));
    assert!(parse(&[
        "--control-listen",
        "127.0.0.1:81",
        "--control-token",
        "t"
    ]));
    assert!(!parse(&["--control-token", "t"]));
    assert!(!parse(&["--control-listen", "127.0.0.1:81"]));
}

struct Published(String);

impl velnor_local::io::env::Engine for Published {
    fn docker(
        &self,
        args: &[&str],
    ) -> impl std::future::Future<Output = std::io::Result<String>> + Send {
        let answer = match args {
            ["port", ..] => self.0.clone(),
            _ => String::new(),
        };
        std::future::ready(Ok(answer))
    }

    fn logged(
        &self,
        _: &[&str],
        _: &std::path::Path,
    ) -> impl std::future::Future<Output = std::io::Result<bool>> + Send {
        std::future::ready(Ok(true))
    }
}

async fn service_up_after(delay: std::time::Duration) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let ready = tokio::time::Instant::now() + delay;
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = [0_u8; 2048];
            let _ = stream.read(&mut buffer).await;
            let status = if tokio::time::Instant::now() >= ready {
                "200 OK"
            } else {
                "503 Service Unavailable"
            };
            let reply =
                format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            let _ = stream.write_all(reply.as_bytes()).await;
        }
    });
    address.to_string()
}

#[tokio::test]
async fn up_waits_for_runner_server_and_the_cache_even_when_no_plain_runner_starts() {
    let dir = temp_dir("environment-pair-ready");
    let delay = std::time::Duration::from_millis(700);
    let address = service_up_after(delay).await;
    let env = Environment::at(&dir, Published(address))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut args = plain(&dir, 1, 0);
    args.runner = RunnerMode::Pair;
    let started = tokio::time::Instant::now();
    let up = env.up(&dir, &args).await;
    assert!(up.is_ok(), "{:?}", up.err());
    assert!(
        started.elapsed() >= delay,
        "up returned before the service answered"
    );
}

async fn stalling_service() -> String {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"));
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = [0_u8; 2048];
            let _ = stream.read(&mut buffer).await;
            held.push(stream);
        }
    });
    address.to_string()
}

#[tokio::test(start_paused = true)]
async fn up_fails_at_the_readiness_deadline_when_the_upstream_never_answers() {
    let dir = temp_dir("environment-pair-stalled");
    let address = stalling_service().await;
    let env = Environment::at(&dir, Published(address))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut args = plain(&dir, 1, 0);
    args.runner = RunnerMode::Pair;
    let limit = std::time::Duration::from_secs(70);
    let up = tokio::time::timeout(limit, env.up(&dir, &args)).await;
    let error = up
        .unwrap_or_else(|_| unreachable!("up hung past the readiness deadline"))
        .err()
        .unwrap_or_else(|| unreachable!("up succeeded against a stalled upstream"));
    assert!(error.to_string().contains("did not connect"), "{error}");
}

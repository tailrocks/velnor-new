// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

use velnor_local::cli::EnvironmentArgs;
use velnor_local::event::{PullRequest, Push, Repository};
use velnor_local::io::env::{Engine, Environment};
use velnor_local::io::{guarded, open};

use crate::support::temp_dir;

#[derive(Debug, Default)]
struct FakeDocker {
    labelled: Mutex<BTreeMap<String, String>>,
    calls: Mutex<Vec<Vec<String>>>,
}

impl FakeDocker {
    fn left(&self) -> Vec<String> {
        let labelled = self.labelled.lock().unwrap_or_else(|e| unreachable!("{e}"));
        labelled.keys().cloned().collect()
    }

    fn record(&self, args: &[&str]) {
        let mut calls = self.calls.lock().unwrap_or_else(|e| unreachable!("{e}"));
        calls.push(args.iter().map(|arg| (*arg).to_owned()).collect());
    }

    fn started(&self) -> Vec<Vec<String>> {
        let calls = self.calls.lock().unwrap_or_else(|e| unreachable!("{e}"));
        calls
            .iter()
            .filter(|call| call.starts_with(&["run".to_owned(), "-d".to_owned()]))
            .cloned()
            .collect()
    }
}

fn after<'a>(call: &'a [String], flag: &str) -> Vec<&'a str> {
    call.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
        .collect()
}

impl Engine for &FakeDocker {
    fn docker(&self, args: &[&str]) -> impl Future<Output = io::Result<String>> + Send {
        self.record(args);
        std::future::ready(self.answer(args))
    }

    fn logged(&self, args: &[&str], _: &Path) -> impl Future<Output = io::Result<bool>> + Send {
        self.record(args);
        std::future::ready(Ok(true))
    }
}

impl FakeDocker {
    fn answer(&self, args: &[&str]) -> io::Result<String> {
        let mut labelled = self.labelled.lock().unwrap_or_else(|e| unreachable!("{e}"));
        let after = |flag: &str| {
            args.iter()
                .position(|arg| *arg == flag)
                .and_then(|at| args.get(at + 1))
                .map(|value| (*value).to_owned())
        };
        let kind = |prefix: &str| -> String {
            let filter = after("--filter").unwrap_or_default();
            let label = filter.trim_start_matches("label=");
            let matching = labelled
                .iter()
                .filter(|(name, value)| name.starts_with(prefix) && value.as_str() == label);
            matching
                .map(|(name, _)| name.trim_start_matches(prefix).to_owned())
                .collect::<Vec<_>>()
                .join("\n")
        };
        match args {
            ["network", "create", ..] => {
                let name = args.last().copied().unwrap_or_default();
                labelled.insert(
                    format!("network:{name}"),
                    after("--label").unwrap_or_default(),
                );
            }
            ["run", "-d", ..] => {
                let name = after("--name").unwrap_or_default();
                labelled.insert(
                    format!("container:{name}"),
                    after("--label").unwrap_or_default(),
                );
            }
            ["port", ..] => return Err(io::Error::other("no public port")),
            ["ps", ..] => return Ok(kind("container:")),
            ["network", "ls", ..] => return Ok(kind("network:")),
            ["rm", "-f", "-v", ids @ ..] => {
                for id in ids {
                    labelled.remove(&format!("container:{id}"));
                }
            }
            ["network", "rm", ids @ ..] => {
                for id in ids {
                    labelled.remove(&format!("network:{id}"));
                }
            }
            _ => {}
        }
        Ok(String::new())
    }
}

#[tokio::test]
async fn an_environment_that_fails_to_start_leaves_no_container_or_network() {
    let dir = temp_dir("environment-fails");
    let docker = FakeDocker::default();
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let args = EnvironmentArgs {
        dir: dir.clone(),
        runners: 2_u16,
        fork_runners: 1,
        default_branch: "main".to_owned(),
    };
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
    let args = EnvironmentArgs {
        dir: dir.clone(),
        runners: 1_u16,
        fork_runners: 1,
        default_branch: "main".to_owned(),
    };
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

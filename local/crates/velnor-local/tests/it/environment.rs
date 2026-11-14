// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::sync::Mutex;

use velnor_local::cli::EnvironmentArgs;
use velnor_local::io::env::{Engine, Environment};
use velnor_local::io::guarded;

use crate::support::temp_dir;

#[derive(Debug, Default)]
struct FakeDocker {
    labelled: Mutex<BTreeMap<String, String>>,
}

impl FakeDocker {
    fn left(&self) -> Vec<String> {
        let labelled = self.labelled.lock().unwrap_or_else(|e| unreachable!("{e}"));
        labelled.keys().cloned().collect()
    }
}

impl Engine for &FakeDocker {
    fn docker(&self, args: &[&str]) -> impl Future<Output = io::Result<String>> + Send {
        std::future::ready(self.answer(args))
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

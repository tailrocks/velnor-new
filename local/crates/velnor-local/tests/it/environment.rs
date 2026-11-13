// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::sync::Mutex;

use velnor_local::cli::EnvironmentArgs;
use velnor_local::io::env::{Engine, Environment};

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
        runners: 2,
        default_branch: "main".to_owned(),
    };
    let started = env.up(&dir, &args).await;
    assert!(started.is_err());
    assert_eq!(docker.left(), Vec::<String>::new());
}

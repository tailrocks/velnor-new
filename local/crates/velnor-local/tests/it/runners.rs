// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::path::Path;

use velnor_local::io::authority::Runners;
use velnor_local::io::env::{Engine, Environment};

use crate::support::temp_dir;

struct Scripted {
    fork_listed: io::Result<&'static str>,
}

impl Engine for Scripted {
    fn docker(&self, args: &[&str]) -> impl Future<Output = io::Result<String>> + Send {
        let answer = match args {
            ["ps", "-aq", "--filter", filter] if filter.ends_with("-fork-cache$") => self
                .fork_listed
                .as_ref()
                .map(|listed| (*listed).to_owned())
                .map_err(|error| io::Error::other(error.to_string())),
            ["port", container, "81/tcp"] if !container.ends_with("-fork-cache") => {
                Ok("127.0.0.1:41000".to_owned())
            }
            ["port", ..] => Err(io::Error::other("no public port")),
            _ => Ok(String::new()),
        };
        std::future::ready(answer)
    }

    fn logged(&self, _: &[&str], _: &Path) -> impl Future<Output = io::Result<bool>> + Send {
        std::future::ready(Ok(true))
    }
}

async fn open(name: &str, fork_listed: io::Result<&'static str>) -> io::Result<()> {
    let dir = temp_dir(name);
    std::fs::create_dir_all(dir.join("keys")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("keys/control"), "token").unwrap_or_else(|e| unreachable!("{e}"));
    let env = Environment::at(&dir, Scripted { fork_listed }).await?;
    Runners::open(env).await.map(|_| ())
}

#[tokio::test]
async fn a_missing_fork_side_opens_the_authority_with_only_the_trusted_side() {
    assert!(open("runners-no-fork", Ok("")).await.is_ok());
}

#[tokio::test]
async fn a_docker_error_while_looking_for_the_fork_side_fails_the_authority_start() {
    let failed = open(
        "runners-docker-down",
        Err(io::Error::other("daemon is down")),
    )
    .await;
    let message = failed
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(message.contains("daemon is down"), "{message}");
}

#[tokio::test]
async fn a_fork_container_that_publishes_no_port_fails_the_authority_start() {
    let failed = open("runners-fork-stopped", Ok("abc123")).await;
    let message = failed
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(message.contains("no public port"), "{message}");
}

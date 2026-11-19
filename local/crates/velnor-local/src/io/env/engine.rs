// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub trait Engine: Sync {
    fn docker(&self, args: &[&str]) -> impl Future<Output = io::Result<String>> + Send;
    fn logged(&self, args: &[&str], log: &Path) -> impl Future<Output = io::Result<bool>> + Send;
}

#[derive(Debug)]
pub struct DockerCli;

impl Engine for DockerCli {
    async fn docker(&self, args: &[&str]) -> io::Result<String> {
        let stdout = program("docker", args, None).await?;
        Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
    }

    async fn logged(&self, args: &[&str], log: &Path) -> io::Result<bool> {
        let log = std::fs::File::create(log)?;
        let status = Command::new("docker")
            .args(args)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .status()
            .await
            .map_err(|error| spawn_error("docker", &error))?;
        Ok(status.success())
    }
}

pub(in crate::io) fn words(text: &'static str) -> Vec<&'static str> {
    text.split_whitespace().collect()
}

fn spawn_error(program: &str, error: &io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("cannot start `{program}`: {error}"))
}

pub(in crate::io) async fn program(
    program: &str,
    args: &[&str],
    input: Option<&[u8]>,
) -> io::Result<Vec<u8>> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(input.map_or_else(Stdio::null, |_| Stdio::piped()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| spawn_error(program, &error))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(input).await?;
    }
    let output = child.wait_with_output().await?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let error = String::from_utf8_lossy(&output.stderr);
    let failed = format!("{program} {}: {}", args.join(" "), error.trim());
    Err(io::Error::other(failed))
}

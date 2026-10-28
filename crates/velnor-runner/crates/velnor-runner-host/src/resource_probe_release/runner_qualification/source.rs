// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::time::timeout;

use crate::error::HostError;

const GIT_TIMEOUT: Duration = Duration::from_secs(5);
const GIT_OUTPUT_LIMIT: usize = 1024 * 1024;
const LOCK_LIMIT: usize = 64 * 1024;

pub(crate) struct SourceSnapshot {
    pub(crate) root: PathBuf,
    pub(crate) source_sha: String,
    pub(crate) lock_bytes: Vec<u8>,
    pub(crate) runner_version: String,
    pub(crate) docker_version: String,
}

#[derive(Deserialize)]
struct RunnerVersions {
    runner_version: String,
    docker_version: String,
}

struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
}

pub(crate) async fn load(root: &Path, source_sha: &str) -> Result<SourceSnapshot, HostError> {
    if !is_lower_hex(source_sha, 40) || !root.is_absolute() {
        return Err(HostError::Identity);
    }
    let canonical = root.canonicalize().map_err(|_| HostError::Path)?;
    if canonical != root {
        return Err(HostError::Path);
    }
    validate_source(&canonical, source_sha).await?;
    let lock_bytes = read_commit_lock(&canonical, source_sha).await?;
    require_worktree_lock(&canonical, &lock_bytes)?;
    let versions = parse_versions(&lock_bytes)?;
    Ok(SourceSnapshot {
        root: canonical,
        source_sha: source_sha.to_owned(),
        lock_bytes,
        runner_version: versions.runner_version,
        docker_version: versions.docker_version,
    })
}

pub(crate) async fn revalidate(snapshot: &SourceSnapshot) -> Result<(), HostError> {
    validate_source(&snapshot.root, &snapshot.source_sha).await?;
    let commit_lock = read_commit_lock(&snapshot.root, &snapshot.source_sha).await?;
    if commit_lock != snapshot.lock_bytes {
        return Err(HostError::Identity);
    }
    require_worktree_lock(&snapshot.root, &snapshot.lock_bytes)?;
    Ok(())
}

async fn validate_source(root: &Path, source_sha: &str) -> Result<(), HostError> {
    let top = git(root, &["rev-parse", "--show-toplevel"]).await?;
    require_success(&top)?;
    let top = one_line(&top.stdout)?;
    if Path::new(top) != root {
        return Err(HostError::Identity);
    }
    let head = git(root, &["rev-parse", "--verify", "HEAD^{commit}"]).await?;
    require_success(&head)?;
    if one_line(&head.stdout)? != source_sha {
        return Err(HostError::Identity);
    }
    let index = git(root, &["ls-files", "-v", "-z"]).await?;
    require_success(&index)?;
    validate_index_flags(&index.stdout)?;
    let diff = git(
        root,
        &[
            "diff",
            "--quiet",
            "--no-ext-diff",
            "--no-textconv",
            "HEAD",
            "--",
        ],
    )
    .await?;
    if !diff.status.success() {
        return Err(HostError::Identity);
    }
    let untracked = git(root, &["ls-files", "--others", "--exclude-standard", "-z"]).await?;
    if !untracked.status.success() || !untracked.stdout.is_empty() {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_index_flags(output: &[u8]) -> Result<(), HostError> {
    let entries = output.strip_suffix(b"\0").ok_or(HostError::Identity)?;
    if entries.is_empty() {
        return Err(HostError::Identity);
    }
    for entry in entries.split(|byte| *byte == 0) {
        if entry.len() < 3 || entry[1] != b' ' || entry[0] != b'H' {
            return Err(HostError::Identity);
        }
    }
    Ok(())
}

async fn read_commit_lock(root: &Path, source_sha: &str) -> Result<Vec<u8>, HostError> {
    let object = format!("{source_sha}:.velnor/runner.lock");
    let output = git(root, &["show", &object]).await?;
    require_success(&output)?;
    if output.stdout.len() > LOCK_LIMIT {
        return Err(HostError::Frame);
    }
    Ok(output.stdout)
}

fn require_worktree_lock(root: &Path, committed: &[u8]) -> Result<(), HostError> {
    if read_lock(root)? == committed {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

fn require_success(output: &CommandOutput) -> Result<(), HostError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn git(root: &Path, args: &[&str]) -> Result<CommandOutput, HostError> {
    let path = std::env::var_os("PATH").ok_or(HostError::Identity)?;
    let mut command = Command::new("git");
    command
        .env_clear()
        .env("PATH", path)
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-C",
        ])
        .arg(root)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    capture(command, GIT_TIMEOUT).await
}

async fn capture(mut command: Command, limit: Duration) -> Result<CommandOutput, HostError> {
    let mut child = command.spawn().map_err(|_| HostError::Identity)?;
    let stdout = child.stdout.take().ok_or(HostError::Identity)?;
    let stderr = child.stderr.take().ok_or(HostError::Identity)?;
    let output = capture_child(&mut child, stdout, stderr);
    match timeout(limit, output).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => {
            stop_child(&mut child).await;
            Err(error)
        }
        Err(_) => {
            stop_child(&mut child).await;
            Err(HostError::DockerTimeout)
        }
    }
}

async fn capture_child<R, E>(
    child: &mut Child,
    stdout: R,
    stderr: E,
) -> Result<CommandOutput, HostError>
where
    R: AsyncRead + Unpin,
    E: AsyncRead + Unpin,
{
    let stdout = read_capped(stdout, GIT_OUTPUT_LIMIT);
    let stderr = read_capped(stderr, GIT_OUTPUT_LIMIT);
    let status = async { child.wait().await.map_err(|_| HostError::Identity) };
    let (stdout, _stderr, status) = tokio::try_join!(stdout, stderr, status)?;
    Ok(CommandOutput { status, stdout })
}

async fn read_capped<R: AsyncRead + Unpin>(
    mut stream: R,
    limit: usize,
) -> Result<Vec<u8>, HostError> {
    let mut output = Vec::with_capacity(limit.min(4096));
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| HostError::Identity)?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > limit {
            return Err(HostError::Frame);
        }
        output.extend_from_slice(&chunk[..read]);
    }
}

async fn stop_child(child: &mut Child) {
    drop(child.kill().await);
    drop(timeout(Duration::from_secs(1), child.wait()).await);
}

fn one_line(bytes: &[u8]) -> Result<&str, HostError> {
    let line = bytes.strip_suffix(b"\n").ok_or(HostError::Identity)?;
    if line.contains(&b'\n') || line.contains(&b'\r') {
        return Err(HostError::Identity);
    }
    std::str::from_utf8(line).map_err(|_| HostError::Identity)
}

fn read_lock(root: &Path) -> Result<Vec<u8>, HostError> {
    super::super::files::read_bounded_regular(&root.join(".velnor/runner.lock"), LOCK_LIMIT)
}

fn parse_versions(bytes: &[u8]) -> Result<RunnerVersions, HostError> {
    let text = std::str::from_utf8(bytes).map_err(|_| HostError::Identity)?;
    let versions: RunnerVersions = toml::from_str(text).map_err(|_| HostError::Identity)?;
    if versions.runner_version.is_empty()
        || versions.docker_version.is_empty()
        || versions.runner_version.trim() != versions.runner_version
        || versions.docker_version.trim() != versions.docker_version
    {
        return Err(HostError::Identity);
    }
    Ok(versions)
}

pub(super) fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

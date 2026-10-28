// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const RUNNER_LOCK: &str = include_str!("../../../../../../.velnor/runner.lock");

pub(crate) struct GitFixture {
    temp: TempDir,
    source_sha: String,
    parent_sha: String,
}

impl GitFixture {
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        Self::create(true)
    }

    pub(crate) fn new_with_ignored_lock() -> Result<Self, Box<dyn Error>> {
        Self::create(false)
    }

    fn create(include_lock: bool) -> Result<Self, Box<dyn Error>> {
        let temp_root = std::env::temp_dir().canonicalize()?;
        let temp = tempfile::Builder::new()
            .prefix("velnor-runner-qualifier-")
            .tempdir_in(temp_root)?;
        run_git(temp.path(), &["init", "--quiet"])?;
        run_git(temp.path(), &["config", "user.name", "Fixture"])?;
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.invalid"],
        )?;
        fs::create_dir(temp.path().join(".velnor"))?;
        fs::write(temp.path().join(".velnor/runner.lock"), RUNNER_LOCK)?;
        fs::write(temp.path().join("tracked.txt"), "parent fixture\n")?;
        let tracked = if include_lock {
            vec!["add", ".velnor/runner.lock", "tracked.txt"]
        } else {
            fs::write(temp.path().join(".gitignore"), ".velnor/runner.lock\n")?;
            vec!["add", ".gitignore", "tracked.txt"]
        };
        run_git(temp.path(), &tracked)?;
        run_git(temp.path(), &["commit", "--quiet", "-m", "parent fixture"])?;
        let parent_sha = String::from_utf8(run_git_output(
            temp.path(),
            &["rev-parse", "--verify", "HEAD^{commit}"],
        )?)?
        .trim()
        .to_owned();
        fs::write(temp.path().join("head.txt"), "head fixture\n")?;
        run_git(temp.path(), &["add", "head.txt"])?;
        run_git(temp.path(), &["commit", "--quiet", "-m", "head fixture"])?;
        let source_sha = String::from_utf8(run_git_output(
            temp.path(),
            &["rev-parse", "--verify", "HEAD^{commit}"],
        )?)?
        .trim()
        .to_owned();
        Ok(Self {
            temp,
            source_sha,
            parent_sha,
        })
    }

    pub(crate) fn root(&self) -> &Path {
        self.temp.path()
    }

    pub(crate) fn source_sha(&self) -> &str {
        &self.source_sha
    }

    pub(crate) fn parent_sha(&self) -> &str {
        &self.parent_sha
    }

    pub(crate) fn change_lock(&self) -> Result<(), Box<dyn Error>> {
        let path = self.root().join(".velnor/runner.lock");
        let mut bytes = fs::read(&path)?;
        bytes.extend_from_slice(b"\nfixture_change = true\n");
        fs::write(path, bytes)?;
        Ok(())
    }

    pub(crate) fn change_tracked(&self) -> Result<(), Box<dyn Error>> {
        fs::write(self.root().join("tracked.txt"), "modified fixture\n")?;
        Ok(())
    }

    pub(crate) fn add_untracked(&self) -> Result<(), Box<dyn Error>> {
        fs::write(self.root().join("untracked.txt"), "untracked\n")?;
        Ok(())
    }

    pub(crate) fn hide_lock_change(&self, index_flag: &str) -> Result<(), Box<dyn Error>> {
        run_git(
            self.root(),
            &["update-index", index_flag, ".velnor/runner.lock"],
        )?;
        self.change_lock()
    }
}

pub(crate) fn invoke(
    root: &Path,
    source_sha: &str,
    runner_id: &str,
    dind_id: &str,
    socket: &Path,
) -> Result<Output, Box<dyn Error>> {
    let child = Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
        .args([
            "qualify-runner-images",
            root.to_str().ok_or("root path is not UTF-8")?,
            source_sha,
            runner_id,
            dind_id,
            socket.to_str().ok_or("socket path is not UTF-8")?,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    wait_child(child, Duration::from_secs(40))
}

pub(crate) fn invoke_with_timeout(
    root: &Path,
    source_sha: &str,
    runner_id: &str,
    dind_id: &str,
    socket: &Path,
    limit: Duration,
) -> Result<Output, Box<dyn Error>> {
    invoke_with_timeout_timed(root, source_sha, runner_id, dind_id, socket, limit)
        .map(|(output, _)| output)
}

pub(crate) fn invoke_with_timeout_timed(
    root: &Path,
    source_sha: &str,
    runner_id: &str,
    dind_id: &str,
    socket: &Path,
    limit: Duration,
) -> Result<(Output, Duration), Box<dyn Error>> {
    let started = Instant::now();
    let child = Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
        .args([
            "qualify-runner-images",
            root.to_str().ok_or("root path is not UTF-8")?,
            source_sha,
            runner_id,
            dind_id,
            socket.to_str().ok_or("socket path is not UTF-8")?,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = wait_child(child, limit)?;
    Ok((output, started.elapsed()))
}

fn wait_child(mut child: Child, limit: Duration) -> Result<Output, Box<dyn Error>> {
    let end = Instant::now() + limit;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }
        if Instant::now() >= end {
            child.kill()?;
            drop(child.wait());
            return Err("qualifier process exceeded its watchdog".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn run_git(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

fn run_git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>, Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

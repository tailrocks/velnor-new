// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const RUNNER_LOCK: &str = include_str!("../../../../../../.velnor/runner.lock");

#[derive(Clone, Copy)]
pub(super) enum HiddenIndexFlag {
    SkipWorktree,
    AssumeUnchanged,
}

impl HiddenIndexFlag {
    fn argument(self) -> &'static str {
        match self {
            Self::SkipWorktree => "--skip-worktree",
            Self::AssumeUnchanged => "--assume-unchanged",
        }
    }
}

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
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700))?;
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

    pub(crate) fn add_replacement_ref(
        &self,
        checkout_replacement: bool,
    ) -> Result<(), Box<dyn Error>> {
        fs::write(self.root().join("tracked.txt"), "replacement source B\n")?;
        let lock_path = self.root().join(".velnor/runner.lock");
        let mut lock = fs::read(&lock_path)?;
        lock.extend_from_slice(b"fixture_marker = \"replacement-b\"\n");
        fs::write(lock_path, lock)?;
        run_git(self.root(), &["add", "--all"])?;
        let tree = String::from_utf8(run_git_output(self.root(), &["write-tree"])?)?
            .trim()
            .to_owned();
        let replacement = String::from_utf8(run_git_output(
            self.root(),
            &["commit-tree", &tree, "-m", "unrelated replacement source"],
        )?)?
        .trim()
        .to_owned();
        run_git(
            self.root(),
            &["checkout", "--quiet", &self.source_sha, "--", "."],
        )?;
        if checkout_replacement {
            run_git(
                self.root(),
                &["checkout", "--quiet", "--detach", &replacement],
            )?;
            run_git(
                self.root(),
                &["update-ref", "--no-deref", "HEAD", &self.source_sha],
            )?;
        }
        run_git(self.root(), &["replace", &self.source_sha, &replacement])?;
        Ok(())
    }

    pub(crate) fn hide_lock_change(&self, index_flag: &str) -> Result<(), Box<dyn Error>> {
        run_git(
            self.root(),
            &["update-index", index_flag, ".velnor/runner.lock"],
        )?;
        self.change_lock()
    }

    pub(crate) fn hide_tracked_skip_worktree_change(&self) -> Result<(), Box<dyn Error>> {
        mutate_tracked_file(self.root(), HiddenIndexFlag::SkipWorktree)
    }

    pub(crate) fn hide_tracked_assume_unchanged_change(&self) -> Result<(), Box<dyn Error>> {
        mutate_tracked_file(self.root(), HiddenIndexFlag::AssumeUnchanged)
    }

    pub(crate) fn diff_head_is_clean(&self) -> Result<bool, Box<dyn Error>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.root())
            .args(["diff", "--quiet", "HEAD", "--"])
            .output()?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(format!(
                "git diff --quiet HEAD failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into()),
        }
    }
}

pub(super) fn mutate_tracked_file(
    root: &Path,
    flag: HiddenIndexFlag,
) -> Result<(), Box<dyn Error>> {
    let canonical = root.canonicalize()?;
    let temp_root = std::env::temp_dir().canonicalize()?;
    let name = canonical.file_name().and_then(|name| name.to_str());
    let root_metadata = fs::symlink_metadata(&canonical)?;
    if canonical != root
        || canonical.parent() != Some(temp_root.as_path())
        || !name.is_some_and(|name| name.starts_with("velnor-runner-qualifier-"))
        || !root_metadata.is_dir()
        || root_metadata.file_type().is_symlink()
        || root_metadata.uid() != rustix::process::geteuid().as_raw()
        || root_metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(format!(
            "hidden-index mutation requires a private fixture root: root={} temp={} name={name:?} uid={} mode={:o}",
            canonical.display(),
            temp_root.display(),
            root_metadata.uid(),
            root_metadata.permissions().mode() & 0o777
        )
        .into());
    }
    let git_dir = canonical.join(".git");
    let git_metadata = fs::symlink_metadata(&git_dir)?;
    let tracked = canonical.join("tracked.txt");
    let tracked_metadata = fs::symlink_metadata(&tracked)?;
    if !git_metadata.is_dir()
        || git_metadata.file_type().is_symlink()
        || !tracked_metadata.is_file()
        || tracked_metadata.file_type().is_symlink()
        || tracked_metadata.uid() != rustix::process::geteuid().as_raw()
        || tracked_metadata.nlink() != 1
        || tracked.canonicalize()? != tracked
        || fs::read(&tracked)? != b"parent fixture\n"
    {
        return Err("hidden-index mutation target is not the fixture file".into());
    }
    let path = std::env::var_os("PATH").ok_or("PATH is unavailable")?;
    let output = Command::new("git")
        .env_clear()
        .env("PATH", path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(&canonical)
        .args(["update-index", flag.argument(), "--", "tracked.txt"])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git update-index failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    fs::write(&tracked, b"changed during qualification\n")?;
    let after = fs::symlink_metadata(&tracked)?;
    if !after.is_file()
        || after.file_type().is_symlink()
        || after.uid() != rustix::process::geteuid().as_raw()
        || after.nlink() != 1
        || after.dev() != tracked_metadata.dev()
        || after.ino() != tracked_metadata.ino()
    {
        return Err("hidden-index mutation changed the fixture file identity".into());
    }
    Ok(())
}

pub(crate) fn invoke(
    root: &Path,
    source_sha: &str,
    runner_id: &str,
    dind_id: &str,
    socket: &Path,
) -> Result<Output, Box<dyn Error>> {
    let child = Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
        .env_remove("GIT_NO_REPLACE_OBJECTS")
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
        .env_remove("GIT_NO_REPLACE_OBJECTS")
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
        .env("GIT_NO_REPLACE_OBJECTS", "1")
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
        .env("GIT_NO_REPLACE_OBJECTS", "1")
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

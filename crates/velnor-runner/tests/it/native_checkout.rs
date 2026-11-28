// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use uuid::Uuid;

use crate::exec_support::{Ran, Scratch};

const TOKEN: &str = "checkout-token-value";

pub(crate) fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CEILING_DIRECTORIES",
            dir.parent()
                .and_then(|parent| parent.canonicalize().ok())
                .unwrap_or_default(),
        )
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .context("run git")?;
    if !output.status.success() {
        bail!(
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(crate) fn commit(work: &Path, file: &str) -> Result<String> {
    std::fs::write(work.join(file), file)?;
    git(work, &["add", file])?;
    git(work, &["commit", "-q", "-m", file])?;
    git(work, &["rev-parse", "HEAD"])
}

pub(crate) struct Rig {
    pub(crate) root: PathBuf,
    pub(crate) head: String,
    pub(crate) merge: String,
}

impl Rig {
    pub(crate) fn new() -> Result<Self> {
        let root =
            Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("checkout-{}", Uuid::new_v4()));
        let work = root.join("work");
        let upstream = root.join("up/tailrocks/velnor-new.git");
        std::fs::create_dir_all(&work)?;
        std::fs::create_dir_all(&upstream)?;
        git(&upstream, &["init", "-q", "--bare"])?;
        git(&work, &["init", "-q", "-b", "main"])?;
        commit(&work, "first")?;
        git(&work, &["branch", "feature"])?;
        git(&work, &["checkout", "-q", "feature"])?;
        commit(&work, "feature")?;
        git(&work, &["checkout", "-q", "main"])?;
        let head = commit(&work, "second")?;
        git(&work, &["merge", "-q", "--no-ff", "-m", "merge", "feature"])?;
        let merge = git(&work, &["rev-parse", "HEAD"])?;
        git(&work, &["reset", "-q", "--hard", &head])?;
        let target = upstream.to_string_lossy().into_owned();
        git(&work, &["push", "-q", &target, "main"])?;
        git(
            &work,
            &["push", "-q", &target, &format!("{merge}:refs/pull/7/merge")],
        )?;
        crate::native_checkout_env::install_wrapper(&root)?;
        Ok(Self { root, head, merge })
    }

    pub(crate) fn advance(&self, file: &str) -> Result<String> {
        let work = self.root.join("work");
        let sha = commit(&work, file)?;
        let upstream = self.root.join("up/tailrocks/velnor-new.git");
        git(&work, &["push", "-q", &upstream.to_string_lossy(), "main"])?;
        Ok(sha)
    }

    pub(crate) fn server_url(&self) -> String {
        format!("file://{}", self.root.join("up").display())
    }

    pub(crate) fn store(&self) -> PathBuf {
        self.root.join("store")
    }

    pub(crate) fn mirror(&self) -> PathBuf {
        self.mirror_of(1)
    }

    pub(crate) fn mirror_of(&self, repository_id: u64) -> PathBuf {
        crate::native_checkout_layout::mirror_in(&self.store(), &self.server_url(), repository_id)
    }

    pub(crate) fn path(&self) -> String {
        format!(
            "{}:{}",
            self.root.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    pub(crate) fn trace(&self) -> PathBuf {
        self.root.join("trace.json")
    }

    pub(crate) fn job(&self, fixture: &str, sha: &str, server_url: &str) -> Result<PathBuf> {
        self.job_edit(fixture, sha, server_url, &[])
    }

    pub(crate) fn job_edit(
        &self,
        fixture: &str,
        sha: &str,
        server_url: &str,
        edits: &[(&str, &str)],
    ) -> Result<PathBuf> {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/exec")
            .join(format!("{fixture}.json"));
        let mut body = std::fs::read_to_string(source)?
            .replace("@SHA@", sha)
            .replace("@SERVER_URL@", server_url);
        for (from, to) in edits {
            body = body.replace(from, to);
        }
        let path = self.root.join(format!("{fixture}-{}.json", Uuid::new_v4()));
        std::fs::write(&path, body)?;
        Ok(path)
    }

    pub(crate) fn start(&self, job: &Path, scratch: &Scratch) -> Result<Child> {
        self.start_env(job, scratch, &[])
    }

    pub(crate) fn start_env(
        &self,
        job: &Path,
        scratch: &Scratch,
        env: &[(&str, &str)],
    ) -> Result<Child> {
        self.start_at(job, &scratch.workspace(), scratch, env)
    }

    fn command(
        &self,
        job: &Path,
        workspace: &Path,
        scratch: &Scratch,
        env: &[(&str, &str)],
    ) -> Command {
        let mut command = Command::new("sh");
        command
            .current_dir(&self.root)
            .args(["-c", "umask 022; exec \"$0\" \"$@\""])
            .arg(env!("CARGO_BIN_EXE_velnor-runner"))
            .args(["exec", "--job"])
            .arg(job)
            .arg("--workspace")
            .arg(workspace)
            .arg("--temp")
            .arg(scratch.options().temp)
            .env("VELNOR_STORE_ROOT", self.store())
            .env("PATH", self.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .envs(env.iter().copied())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    pub(crate) fn start_at(
        &self,
        job: &Path,
        workspace: &Path,
        scratch: &Scratch,
        env: &[(&str, &str)],
    ) -> Result<Child> {
        self.command(job, workspace, scratch, env)
            .spawn()
            .context("start velnor-runner exec")
    }

    pub(crate) fn start_trusted(
        &self,
        job: &Path,
        facts: &Path,
        scratch: &Scratch,
    ) -> Result<Child> {
        self.command(job, &scratch.workspace(), scratch, &[])
            .arg("--trusted")
            .arg(facts)
            .spawn()
            .context("start velnor-runner exec")
    }

    pub(crate) fn run(&self, fixture: &str, sha: &str, scratch: &Scratch) -> Result<Ran> {
        finish(self.start(&self.job(fixture, sha, &self.server_url())?, scratch)?)
    }

    pub(crate) fn fetches(&self) -> usize {
        self.commands("fetch")
    }

    pub(crate) fn commands(&self, name: &str) -> usize {
        let trace = std::fs::read_to_string(self.trace()).unwrap_or_default();
        let named = format!("\"name\":\"{name}\"");
        trace
            .lines()
            .filter(|line| line.contains("\"event\":\"cmd_name\"") && line.contains(&named))
            .count()
    }

    pub(crate) fn refetches(&self) -> usize {
        std::fs::read_to_string(self.trace())
            .unwrap_or_default()
            .lines()
            .filter(|line| {
                line.contains("\"event\":\"start\"")
                    && line.contains("\"--refetch\"")
                    && !line.contains("refs/velnor/sha/")
            })
            .count()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(crate) fn finish(child: Child) -> Result<Ran> {
    let started = Instant::now();
    let output = child.wait_with_output().context("wait for velnor-runner")?;
    Ok(Ran {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
        elapsed: started.elapsed(),
    })
}

#[test]
fn checkout_depth1_materializes_head_commit() -> Result<()> {
    let rig = Rig::new()?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let workspace = scratch.workspace();
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"])?, rig.head);
    assert!(git(&workspace, &["symbolic-ref", "-q", "HEAD"]).is_err());
    assert_eq!(git(&workspace, &["status", "--porcelain"])?, "");
    assert_eq!(std::fs::read_to_string(workspace.join("second"))?, "second");
    assert_eq!(rig.fetches(), 1);
    Ok(())
}

#[test]
fn history_is_available_through_the_mirror() -> Result<()> {
    let rig = Rig::new()?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth2", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let workspace = scratch.workspace();
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"])?, rig.head);
    assert!(git(&workspace, &["symbolic-ref", "-q", "HEAD"]).is_err());
    assert_eq!(git(&workspace, &["rev-list", "--count", "HEAD"])?, "2");
    Ok(())
}

#[test]
fn warm_second_job_runs_no_fetch() -> Result<()> {
    let rig = Rig::new()?;
    for _ in 0..2 {
        let scratch = Scratch::new()?;
        let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
        assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
        assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    }
    assert_eq!(rig.fetches(), 1);
    Ok(())
}

#[test]
fn concurrent_jobs_serialize_on_mirror_lock() -> Result<()> {
    let rig = Rig::new()?;
    let job = rig.job("checkout_depth1", &rig.head, &rig.server_url())?;
    let scratches = [Scratch::new()?, Scratch::new()?];
    let children = [
        rig.start(&job, &scratches[0])?,
        rig.start(&job, &scratches[1])?,
    ];
    for (child, scratch) in children.into_iter().zip(&scratches) {
        let ran = finish(child)?;
        assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
        assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    }
    assert_eq!(rig.fetches(), 1);
    Ok(())
}

#[test]
fn checkout_rejects_ref_input_with_unsupported_message() -> Result<()> {
    let rig = Rig::new()?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_unsupported_ref", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(1));
    assert!(
        ran.has_line("unsupported: actions/checkout input 'ref' is not supported by velnor-runner"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!scratch.workspace().join(".git").exists());
    assert_eq!(rig.fetches(), 0);
    Ok(())
}

fn find_in_tree(dir: &Path, needles: &[&str]) -> Result<Option<PathBuf>> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if let Some(found) = find_in_tree(&path, needles)? {
                return Ok(Some(found));
            }
        } else if path.is_file() {
            let body = String::from_utf8_lossy(&std::fs::read(&path)?).into_owned();
            if needles.iter().any(|needle| body.contains(needle)) {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

#[test]
fn checkout_never_writes_token_to_git_config() -> Result<()> {
    let rig = Rig::new()?;
    let credential = STANDARD.encode(format!("x-access-token:{TOKEN}"));
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let refused = Scratch::new()?;
    let closed_port = crate::support::ClosedPort::start()?;
    let closed = closed_port.port();
    let url = format!("https://127.0.0.1:{closed}");
    let job = rig.job("checkout_depth1", &"a".repeat(40), &url)?;
    let failed = finish(rig.start(&job, &refused)?)?;
    assert_eq!(failed.code, Some(1), "{}\n{}", failed.stdout, failed.stderr);
    for text in [&ran.stdout, &ran.stderr, &failed.stdout, &failed.stderr] {
        assert!(
            !text.contains(TOKEN) && !text.contains(&credential),
            "{text}"
        );
    }
    assert!(find_in_tree(&rig.store(), &[TOKEN, &credential])?.is_none());
    assert!(find_in_tree(&scratch.workspace().join(".git"), &[TOKEN, &credential])?.is_none());
    assert_eq!(
        git(&scratch.workspace(), &["config", "remote.origin.url"])?,
        format!("{}/tailrocks/velnor-new.git", rig.server_url())
    );
    assert!(!refused.workspace().join(".git").exists());
    Ok(())
}

#[test]
fn checkout_pull_request_fetches_merge_commit() -> Result<()> {
    let rig = Rig::new()?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_pull_request", &rig.merge, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(
        git(&scratch.workspace(), &["rev-parse", "HEAD"])?,
        rig.merge
    );
    assert_eq!(rig.fetches(), 1);
    Ok(())
}

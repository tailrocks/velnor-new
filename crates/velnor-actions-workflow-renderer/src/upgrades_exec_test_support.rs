//! Sandbox that executes the rendered upgrade job scripts for real.
//!
//! Each sandbox is a throwaway Git repository with a local bare `origin`, a
//! stub directory first on `PATH` (fake `mise` and `gh`, and whatever a test
//! adds), and a work directory standing in for `$VELNOR_UPGRADE_DIR`. The
//! scripts run under `bash` with a cleared environment, so nothing reaches
//! the caller's Git config, credentials or tool state.

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::schema2::git_fixture;

pub(super) type TestResult<T = ()> = Result<T, Box<dyn Error>>;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Fake `mise`: the pinned GitHub CLI call runs the fake `gh`; anything else
/// is logged and handled by an optional per-test `mise-impl` script.
const FAKE_MISE: &str = r#"#!/bin/sh
printf 'mise %s\n' "$*" >> "$SANDBOX_LOG"
if [ "$1 $2 $3 $4" = "--no-config --no-env --no-hooks exec" ]; then
  shift 4
  case "$1" in gh@*) shift; [ "$1" = -- ] && shift; exec "$@" ;; esac
fi
if [ -x "$SANDBOX_BIN/mise-impl" ]; then exec "$SANDBOX_BIN/mise-impl" "$@"; fi
exit 0
"#;

/// Fake `gh`: logs every call; `pr list` reports no open pull request.
const FAKE_GH: &str = r#"#!/bin/sh
printf 'gh %s\n' "$*" >> "$SANDBOX_LOG"
if [ -x "$SANDBOX_BIN/gh-impl" ]; then exec "$SANDBOX_BIN/gh-impl" "$@"; fi
case "$1 $2" in
  "pr list") exit 0 ;;
  "pr create") echo https://github.com/o/r/pull/1 ;;
esac
exit 0
"#;

pub(super) struct Sandbox {
    pub(super) root: PathBuf,
    pub(super) repo: PathBuf,
    pub(super) origin: PathBuf,
    pub(super) work: PathBuf,
    pub(super) bin: PathBuf,
    pub(super) log: PathBuf,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.root));
    }
}

impl Sandbox {
    /// A committed repository with upgrade-owned and foreign files, pushed to
    /// `origin/main`.
    pub(super) fn new(name: &str) -> TestResult<Self> {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "velnor-upgrades-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let sandbox = Self {
            repo: root.join("repo"),
            origin: root.join("origin.git"),
            work: root.join("work"),
            bin: root.join("bin"),
            log: root.join("calls.log"),
            root,
        };
        for dir in [&sandbox.repo, &sandbox.work, &sandbox.bin] {
            fs::create_dir_all(dir)?;
        }
        fs::write(&sandbox.log, "")?;
        sandbox.stub("mise", FAKE_MISE)?;
        sandbox.stub("gh", FAKE_GH)?;
        sandbox.init_git()?;
        Ok(sandbox)
    }

    fn init_git(&self) -> TestResult {
        bare_git(
            &self.root,
            &["init", "--quiet", "--bare", "-b", "main", "origin.git"],
        )?;
        bare_git(&self.repo, &["init", "--quiet", "-b", "main"])?;
        for (path, text) in [
            ("mise.toml", "[tools]\nrust = \"1.98.1\"\n"),
            ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.98.1\"\n"),
            (
                "Cargo.toml",
                "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
            ),
            ("Cargo.lock", "version = 4\n"),
            ("src/main.rs", "fn main() {}\n"),
            ("deny.toml", "[advisories]\n"),
            (".github/workflows/ci.yml", "name: CI\n"),
            (
                ".velnor/release-manifest.json",
                "{\"schema\":1,\"version\":\"0.1.0\"}\n",
            ),
        ] {
            self.write(path, text)?;
        }
        self.git(&["add", "-A"])?;
        self.git(&["commit", "--quiet", "-m", "fixture"])?;
        let origin = self.origin.to_string_lossy().into_owned();
        self.git(&["remote", "add", "origin", &origin])?;
        self.git(&["push", "--quiet", "origin", "main"])?;
        Ok(())
    }

    pub(super) fn write(&self, path: &str, text: &str) -> TestResult {
        let path = self.repo.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
        Ok(())
    }

    /// Install an executable stub on the sandbox `PATH`.
    pub(super) fn stub(&self, name: &str, body: &str) -> TestResult {
        let path = self.bin.join(name);
        fs::write(&path, body)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    /// Run fixture Git in the repository and return stdout.
    pub(super) fn git(&self, args: &[&str]) -> TestResult<String> {
        let output = git_fixture::command(&self.repo)?.args(args).output()?;
        if !output.status.success() {
            return Err(
                format!("git {args:?}: {}", String::from_utf8_lossy(&output.stderr)).into(),
            );
        }
        Ok(String::from_utf8(output.stdout)?)
    }

    /// Bytes of a `git` command's stdout (patches may be binary).
    pub(super) fn git_bytes(&self, args: &[&str]) -> TestResult<Vec<u8>> {
        let output = git_fixture::command(&self.repo)?.args(args).output()?;
        if !output.status.success() {
            return Err(format!("git {args:?} failed").into());
        }
        Ok(output.stdout)
    }

    /// Stage whatever the test changed, capture it with `diff_args` as the
    /// upgrade patch, then restore the committed state.
    pub(super) fn capture_patch(&self, diff_args: &[&str]) -> TestResult<PathBuf> {
        self.git(&["add", "-A"])?;
        let mut args = vec!["diff", "--cached"];
        args.extend_from_slice(diff_args);
        let bytes = self.git_bytes(&args)?;
        self.reset()?;
        let patch = self.work.join("upgrade.patch");
        fs::write(&patch, bytes)?;
        fs::write(self.work.join("summary.md"), "- fixture upgrade\n")?;
        Ok(patch)
    }

    pub(super) fn reset(&self) -> TestResult {
        self.git(&["reset", "--quiet", "--hard", "HEAD"])?;
        self.git(&["clean", "--quiet", "-fdx"])?;
        Ok(())
    }

    /// Run a rendered job script in the repository with a cleared
    /// environment plus `env`.
    pub(super) fn run(&self, script: &str, env: &[(&str, &str)]) -> TestResult<Output> {
        let script_path = self.root.join("job.sh");
        fs::write(&script_path, script)?;
        let inherited = std::env::var_os("PATH").ok_or("PATH is unavailable")?;
        let mut paths = vec![self.bin.clone()];
        paths.extend(std::env::split_paths(&inherited));
        let mut command = Command::new("bash");
        command
            .arg(&script_path)
            .current_dir(&self.repo)
            .env_clear()
            .env("PATH", std::env::join_paths(paths)?)
            .env("HOME", &self.root)
            .env("TMPDIR", &self.root)
            .env("SANDBOX_LOG", &self.log)
            .env("SANDBOX_BIN", &self.bin)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("VELNOR_UPGRADE_DIR", &self.work);
        for (key, value) in env {
            command.env(key, value);
        }
        Ok(command.output()?)
    }

    /// Run the publish script against the captured patch.
    pub(super) fn publish(&self) -> TestResult<Output> {
        self.run(
            &super::publish_script(),
            &[
                ("GH_TOKEN", "fixture-app-token"),
                ("VELNOR_UPGRADE_BRANCH", "velnor/upgrades"),
                ("VELNOR_DEFAULT_BRANCH", "main"),
                ("GITHUB_REPOSITORY", "o/r"),
                ("VELNOR_GH_VERSION", "2.102.1"),
            ],
        )
    }

    /// Head of `velnor/upgrades` on origin, if the branch exists.
    pub(super) fn pushed_branch(&self) -> TestResult<Option<String>> {
        let output = git_fixture::sterile_command()
            .arg("--git-dir")
            .arg(&self.origin)
            .args([
                "rev-parse",
                "--verify",
                "--quiet",
                "refs/heads/velnor/upgrades",
            ])
            .output()?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
    }

    pub(super) fn calls(&self) -> TestResult<String> {
        Ok(fs::read_to_string(&self.log)?)
    }
}

fn bare_git(dir: &Path, args: &[&str]) -> TestResult {
    let output = git_fixture::sterile_command()
        .current_dir(dir)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!("git {args:?}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(())
}

/// Combined stdout and stderr of a script run, for assertions.
pub(super) fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

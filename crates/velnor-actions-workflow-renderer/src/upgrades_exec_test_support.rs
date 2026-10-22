//! Sandbox that executes the rendered upgrade job scripts for real.
//!
//! Each sandbox is a throwaway Git repository with a local bare `origin`, a
//! stub directory first on `PATH` (fake `mise` and `gh`, and whatever a test
//! adds), a work directory standing in for `$VELNOR_UPGRADE_DIR`, and a fake
//! `tailrocks/velnor-new` release store: each release carries a stand-in
//! `velnor-actions` (a shell script whose `generate` writes `ci.yml`), its
//! manifest, and "attestations" the fake `gh attestation verify` accepts only
//! for the exact subject digest and source commit. The scripts run under
//! `bash` with a cleared environment, so nothing reaches the caller's Git
//! config, credentials or tool state.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use super::exec_fakes::{FAKE_GH, FAKE_MISE, bare_git, generator, sha256};
pub(super) use super::exec_fakes::{generated_ci, text};
use crate::schema2::git_fixture;

pub(super) type TestResult<T = ()> = Result<T, Box<dyn Error>>;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Source commit every fixture release claims.
pub(super) const RELEASE_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

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

/// Outcome of the verify and publish jobs for one prepared patch.
pub(super) struct Pipeline {
    pub(super) verify: Output,
    pub(super) outputs: BTreeMap<String, String>,
    pub(super) publish: Option<Output>,
}

impl Pipeline {
    pub(super) fn log(&self) -> String {
        let mut log = text(&self.verify);
        if let Some(publish) = &self.publish {
            log.push_str(&text(publish));
        }
        log
    }
}

impl Sandbox {
    /// A committed repository on Velnor 0.1.0 (its manifest and generated
    /// `ci.yml`) with upgrade-owned and foreign files, pushed to
    /// `origin/main`; 0.1.0 is the only fixture release.
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
        sandbox.release("0.1.0")?;
        sandbox.init_git()?;
        Ok(sandbox)
    }

    fn init_git(&self) -> TestResult {
        bare_git(
            &self.root,
            &["init", "--quiet", "--bare", "-b", "main", "origin.git"],
        )?;
        bare_git(&self.repo, &["init", "--quiet", "-b", "main"])?;
        let manifest = fs::read_to_string(self.release_dir("0.1.0").join("release-manifest.json"))?;
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
            (".github/workflows/ci.yml", &generated_ci("0.1.0")),
            (".velnor/release-manifest.json", &manifest),
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

    pub(super) fn release_dir(&self, version: &str) -> PathBuf {
        self.root.join("releases").join(format!("v{version}"))
    }

    /// Publish a fixture release with a valid manifest, binary, digest and
    /// attestations, and list it in the release index.
    pub(super) fn release(&self, version: &str) -> TestResult {
        let dir = self.release_dir(version);
        fs::create_dir_all(&dir)?;
        let name = format!("velnor-actions-{version}-x86_64-unknown-linux-gnu");
        let binary = dir.join(&name);
        fs::write(&binary, generator(version))?;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
        let digest = sha256(&binary)?;
        let manifest = format!(
            "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{RELEASE_COMMIT}\",\"targets\":[{{\"target\":\"x86_64-unknown-linux-gnu\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/{name}\",\"sha256\":\"{digest}\"}}]}}\n"
        );
        fs::write(dir.join("release-manifest.json"), manifest)?;
        self.attest(version, "release-manifest.json")?;
        self.attest(version, &name)?;
        let mut tags: Vec<String> = fs::read_dir(self.root.join("releases"))?
            .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
            .collect::<std::io::Result<_>>()?;
        tags.sort();
        let index = tags
            .iter()
            .map(|tag| format!("{{\"tag_name\":\"{tag}\",\"draft\":false,\"prerelease\":false}}"))
            .collect::<Vec<_>>()
            .join(",");
        fs::write(self.root.join("releases.json"), format!("[{index}]\n"))?;
        Ok(())
    }

    /// (Re)write the fake attestation for one release asset.
    pub(super) fn attest(&self, version: &str, asset: &str) -> TestResult {
        let dir = self.release_dir(version);
        let digest = sha256(&dir.join(asset))?;
        fs::write(
            dir.join(format!("{asset}.intoto.jsonl")),
            format!("attested {digest} {RELEASE_COMMIT}"),
        )?;
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

    /// Make the checkout look like prepare after a Velnor bump to `version`:
    /// the release manifest and the regenerated `ci.yml`.
    pub(super) fn bump_velnor(&self, version: &str) -> TestResult {
        let manifest = fs::read_to_string(self.release_dir(version).join("release-manifest.json"))?;
        self.write(".velnor/release-manifest.json", &manifest)?;
        self.write(".github/workflows/ci.yml", &generated_ci(version))
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
    fn git_bytes(&self, args: &[&str]) -> TestResult<Vec<u8>> {
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

    /// A fresh checkout of the default branch, as every job starts from.
    pub(super) fn reset(&self) -> TestResult {
        self.git(&["reset", "--quiet", "--hard", "HEAD"])?;
        self.git(&["clean", "--quiet", "-fdx"])?;
        Ok(())
    }

    /// Run a rendered job script in the repository with a cleared
    /// environment plus `env`; `GITHUB_OUTPUT` is `root/github-output`.
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
            .env("SANDBOX_ROOT", &self.root)
            .env("GITHUB_OUTPUT", self.root.join("github-output"))
            .env("GITHUB_REPOSITORY", "o/r")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("VELNOR_UPGRADE_DIR", &self.work)
            .env("VELNOR_GH_VERSION", "2.103.0")
            .env(
                "VELNOR_VERIFY_RUST_SCRIPT",
                super::exec_fakes::VERIFY_RUST_STUB,
            )
            .env(
                "VELNOR_GENERATE_TOOLS",
                "jactionlint@2.3.0 shellcheck@0.11.0",
            );
        for (key, value) in env {
            command.env(key, value);
        }
        Ok(command.output()?)
    }

    /// `KEY=value` lines the scripts appended to `GITHUB_OUTPUT`.
    pub(super) fn outputs(&self) -> BTreeMap<String, String> {
        let path = self.root.join("github-output");
        let text = fs::read_to_string(&path).unwrap_or_default();
        fs::remove_file(&path).ok();
        text.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect()
    }

    /// The release step, with the read-only token it holds in the workflow.
    pub(super) fn verify_release(&self) -> TestResult<Output> {
        self.run(super::RELEASE, &[("GH_TOKEN", "fixture-read-token")])
    }

    /// The prepare job: its release step, then the bump script, with the
    /// prepare `mise` stand-in and `env` on top.
    pub(super) fn prepare(&self, env: &[(&str, &str)]) -> TestResult<Output> {
        self.stub("mise-impl", super::exec_fakes::PREPARE_MISE)?;
        let release = self.verify_release()?;
        if !release.status.success() {
            return Ok(release);
        }
        let mut all = vec![
            ("MISE_MINIMUM_RELEASE_AGE", "0s"),
            ("MISE_YES", "1"),
            ("VELNOR_CARGO_EDIT_VERSION", "0.13.13"),
            ("VELNOR_CATALOG_BUMPER_SOURCE", ""),
        ];
        all.extend_from_slice(env);
        self.run(&super::prepare_script(), &all)
    }

    /// Run the verify job, then (only when it accepted the patch) the
    /// publish job's check and push steps, each from a fresh checkout.
    pub(super) fn pipeline(&self) -> TestResult<Pipeline> {
        self.reset()?;
        let release = self.verify_release()?;
        if !release.status.success() {
            return Ok(Pipeline {
                verify: release,
                outputs: BTreeMap::new(),
                publish: None,
            });
        }
        let verify = self.run(&super::verify_script(), &[])?;
        let outputs = self.outputs();
        if !verify.status.success() {
            return Ok(Pipeline {
                verify,
                outputs,
                publish: None,
            });
        }
        self.reset()?;
        let publish = self.publish(&outputs)?;
        Ok(Pipeline {
            verify,
            outputs,
            publish: Some(publish),
        })
    }

    /// The publish job's token-free check, then its push step.
    pub(super) fn publish(&self, outputs: &BTreeMap<String, String>) -> TestResult<Output> {
        let tree = outputs.get("tree").map_or("", String::as_str);
        let sha = outputs.get("patch_sha256").map_or("", String::as_str);
        let verified = [
            ("VELNOR_VERIFIED_TREE", tree),
            ("VELNOR_VERIFIED_PATCH_SHA256", sha),
        ];
        let check = self.run(&super::publish_check_script(), &verified)?;
        if !check.status.success() {
            return Ok(check);
        }
        fs::write(self.root.join("token-minted"), "")?;
        let mut push = verified.to_vec();
        push.extend([
            ("GH_TOKEN", "fixture-app-token"),
            ("VELNOR_UPGRADE_BRANCH", "velnor/upgrades"),
            ("VELNOR_DEFAULT_BRANCH", "main"),
        ]);
        self.run(super::PUBLISH, &push)
    }

    /// Whether the publish job reached its token step.
    pub(super) fn token_minted(&self) -> bool {
        self.root.join("token-minted").exists()
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

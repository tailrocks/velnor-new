// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fmt::Write as _,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::json;
#[path = "operator_toolchain_guard_fixture/mise.rs"]
mod mise;

pub(super) struct Fixture {
    _temp: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) repo: PathBuf,
    pub(super) guard: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let repo = root.join("repo");
        let guard = root.join("bin/guard");
        fs::create_dir_all(repo.join(".velnor"))?;
        fs::create_dir_all(root.join("bin"))?;
        fs::create_dir_all(root.join("home"))?;
        fs::create_dir_all(repo.join("src"))?;
        fs::create_dir_all(repo.join("Formula"))?;
        fs::write(repo.join("Cargo.toml"), CARGO_TOML)?;
        fs::write(
            repo.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"probe\"\nversion = \"0.1.0\"\n",
        )?;
        fs::write(repo.join("src/main.rs"), "fn main() {}\n")?;
        fs::write(
            repo.join("mise.toml"),
            "[tasks.\"release:package\"]\nrun = \"true\"\n\n[tasks.\"tap:update\"]\nrun = \"true\"\n",
        )?;
        fs::write(
            repo.join("Formula/probe.rb"),
            "class Probe < Formula; end\n",
        )?;
        fs::write(
            repo.join(".velnor/config.toml"),
            format!("{CONFIG}\n{QUALIFIED_RUST}"),
        )?;
        Self::write_metadata(&repo, &root)?;
        mise::write(&root, &repo)?;
        Self::write_guard(&guard, "exec \"$@\"\n")?;
        git(&repo, &["init", "-b", "main"])?;
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/example/probe.git",
            ],
        )?;
        git(&repo, &["add", "."])?;
        git(
            &repo,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-m",
                "fixture",
            ],
        )?;
        Ok(Self {
            _temp: temp,
            root,
            repo,
            guard,
        })
    }

    pub(super) fn invoke(&self, args: &[&str]) -> Result<Output, Box<dyn Error>> {
        self.invoke_with_guard_policy(args, false)
    }

    pub(super) fn invoke_with_guard_policy(
        &self,
        args: &[&str],
        require_guard: bool,
    ) -> Result<Output, Box<dyn Error>> {
        let path = std::env::join_paths([
            self.root.join("bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ])?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
        command
            .args(args)
            .current_dir(&self.repo)
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GITHUB_REPOSITORY", "example/probe");
        if require_guard {
            command.env("VELNOR_TEST_GUARD_REQUIRED", "1");
        }
        Ok(command.output()?)
    }

    pub(super) fn generate_unguarded(&self, output: &Path) -> Result<Output, Box<dyn Error>> {
        self.invoke(&[
            "generate",
            "--output-dir",
            output.to_str().ok_or("non-UTF-8 output path")?,
        ])
    }

    pub(super) fn generate_with_guard_policy(
        &self,
        mode: &str,
        output: &Path,
    ) -> Result<Output, Box<dyn Error>> {
        self.write_guard_mode(mode)?;
        self.invoke_with_guard_policy(
            &[
                "--toolchain-guard",
                self.guard.to_str().ok_or("non-UTF-8 guard path")?,
                "generate",
                "--output-dir",
                output.to_str().ok_or("non-UTF-8 output path")?,
            ],
            true,
        )
    }

    pub(super) fn enable_release(&self, registry: &str) -> Result<(), Box<dyn Error>> {
        let config = self.repo.join(".velnor/config.toml");
        let mut contents = fs::read_to_string(&config)?;
        writeln!(
            contents,
            "\n[stacks.rust.release]\nenabled = true\npackages = [\"probe\"]\nregistry = \"{registry}\""
        )?;
        fs::write(config, contents)?;
        Ok(())
    }

    pub(super) fn generate(&self, mode: &str, output: &Path) -> Result<Output, Box<dyn Error>> {
        self.write_guard_mode(mode)?;
        self.generate_with_guard(self.guard.to_str().ok_or("non-UTF-8 guard path")?, output)
    }

    pub(super) fn generate_with_guard(
        &self,
        guard: &str,
        output: &Path,
    ) -> Result<Output, Box<dyn Error>> {
        self.invoke(&[
            "--toolchain-guard",
            guard,
            "generate",
            "--output-dir",
            output.to_str().ok_or("non-UTF-8 output path")?,
        ])
    }

    pub(super) fn source_export(
        &self,
        mode: &str,
        output: &Path,
    ) -> Result<Output, Box<dyn Error>> {
        self.write_guard_mode(mode)?;
        self.invoke(&[
            "--toolchain-guard",
            self.guard.to_str().ok_or("non-UTF-8 guard path")?,
            "homebrew",
            "source-policy",
            "--root",
            self.repo.to_str().ok_or("non-UTF-8 repo path")?,
            "--output",
            output.to_str().ok_or("non-UTF-8 output path")?,
        ])
    }

    pub(super) fn source_export_unguarded(&self, output: &Path) -> Result<Output, Box<dyn Error>> {
        self.invoke(&[
            "homebrew",
            "source-policy",
            "--root",
            self.repo.to_str().ok_or("non-UTF-8 repo path")?,
            "--output",
            output.to_str().ok_or("non-UTF-8 output path")?,
        ])
    }

    pub(super) fn plan(&self, mode: &str) -> Result<Output, Box<dyn Error>> {
        self.write_guard_mode(mode)?;
        self.invoke_with_guard_policy(
            &[
                "--toolchain-guard",
                self.guard.to_str().ok_or("non-UTF-8 guard path")?,
                "plan",
            ],
            true,
        )
    }

    pub(super) fn source_check(&self, mode: &str) -> Result<Output, Box<dyn Error>> {
        self.write_guard_mode(mode)?;
        self.invoke(&[
            "--toolchain-guard",
            self.guard.to_str().ok_or("non-UTF-8 guard path")?,
            "homebrew",
            "source-policy",
            "--root",
            self.repo.to_str().ok_or("non-UTF-8 repo path")?,
            "--check",
        ])
    }

    fn write_guard_mode(&self, mode: &str) -> Result<(), Box<dyn Error>> {
        let body = match mode {
            "deny" => "exit 77\n",
            "allow" => "export VELNOR_TEST_GUARD_ADMITTED=1\nexec \"$@\"\n",
            _ => return Err("unknown guard mode".into()),
        };
        Self::write_guard(&self.guard, body)
    }

    fn write_guard(path: &Path, body: &str) -> Result<(), Box<dyn Error>> {
        fs::write(
            path,
            format!("#!/bin/sh\nset -eu\n[ \"$1\" = \"--\" ] || exit 90\nshift\n{body}"),
        )?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        Ok(())
    }

    fn write_metadata(repo: &Path, root: &Path) -> Result<(), Box<dyn Error>> {
        let package = json!({
            "id": format!("path+file://{}/#probe@0.1.0", repo.display()),
            "name": "probe",
            "version": "0.1.0",
            "manifest_path": repo.join("Cargo.toml"),
            "targets": [{"kind": ["bin"], "name": "probe", "src_path": repo.join("src/main.rs"), "test": true, "doctest": false}],
            "dependencies": [],
            "features": {},
        });
        let metadata = json!({
            "version": 1,
            "workspace_root": repo,
            "workspace_members": [package["id"].clone()],
            "workspace_default_members": [package["id"].clone()],
            "target_directory": repo.join("target"),
            "resolve": {
                "root": package["id"].clone(),
                "nodes": [{"id": package["id"].clone(), "dependencies": [], "deps": [], "features": []}],
            },
            "packages": [package],
        });
        fs::write(
            root.join("bin/metadata.json"),
            serde_json::to_vec(&metadata)?,
        )?;
        Ok(())
    }
}

pub(super) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn git(repo: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "Git fixture setup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

const CARGO_TOML: &str = "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

const CONFIG: &str = r#"schema = 2
[workflow]
name = "CI"
policy = "consumer-v1"
default_branch = "main"
generator_validation = "bootstrap"
max_parallel_jobs = 2
[execution]
default_profile = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
[stacks.rust.binary_release]
manifest_path = "Cargo.toml"
version_package = "probe"
version_env = "PROBE_VERSION_OVERRIDE"
[stacks.rust.binary_release.preview]
enabled = true
[stacks.rust.binary_release.producer]
task = "release:package"
source = { mise_config = "mise.toml", working_directory = "." }
inputs = ["Cargo.toml", "Cargo.lock", "mise.toml"]
tools = ["rust"]
timeout_minutes = 1
output_directory = "dist/release"
manifest = "dist/release/output.json"
[stacks.rust.binary_release.evidence]
profile = "signed-sbom-v1"
[[stacks.rust.binary_release.products]]
id = "probe"
package = "probe"
bins = ["probe"]
targets = ["x86_64-unknown-linux-gnu"]
archive = "tar.gz"
[distribution.homebrew_tap]
enabled = true
repository = "example/homebrew-probe"
environment = "tap-update"
app_id_variable = "TAP_APP_ID"
app_private_key_secret = "TAP_APP_PRIVATE_KEY"
merge = "squash-after-gates"
[distribution.homebrew_tap.destinations.preview]
formula = "Formula/probe.rb"
[distribution.homebrew_tap.updater]
task = "tap:update"
source = { mise_config = "mise.toml", working_directory = "." }
runner = { label = "ubuntu-26.04", platform = "linux_x64", executor = "hosted" }
inputs = ["mise.toml", "Formula/probe.rb"]
tools = ["ruby"]
timeout_minutes = 1
[distribution.homebrew_tap.protection_observer]
app_id_variable = "TAP_OBSERVER_APP_ID"
app_private_key_secret = "TAP_OBSERVER_APP_PRIVATE_KEY"
[distribution.homebrew_tap.review_completion]
check_name = "Review completion"
app_id_variable = "REVIEW_APP_ID"
[[distribution.homebrew_tap.review_completion.expected_reviewers]]
app_id_variable = "CODEX_REVIEWER_APP_ID"
"#;

const QUALIFIED_RUST: &str = r#"[[qualified_tools]]
id = "rust"
backend = { kind = "core", tool = "rust" }
version = "1.99.1"
options = { kind = "rust", components = [], targets = [] }
depends_on = []
[[qualified_tools.platforms]]
platform = "linux_x64"
artifacts = [
  { url = "https://static.rust-lang.org/dist/2026-10-15/cargo-1.99.1-x86_64-unknown-linux-gnu.tar.xz", sha256 = "53f3521949404d595efb06a55c12c5658d4ed1c75566f33d3d514eaebe70bd8a" },
  { url = "https://static.rust-lang.org/dist/2026-10-15/rust-std-1.99.1-x86_64-unknown-linux-gnu.tar.xz", sha256 = "109b074225bf5b9ea3820890876bfdf4ed161adbfaf94b2b4b5984c85ee5d958" },
  { url = "https://static.rust-lang.org/dist/2026-10-15/rustc-1.99.1-x86_64-unknown-linux-gnu.tar.xz", sha256 = "2ff05ff9c08a90861e7f6e4065b40a8b87463b4576186fd613871c3e37f2091f" },
]
dependency_artifacts = []
install_tree_sha256 = "baa8089f830b6c49980b5c2274a0863d963d2eae8ef7f8ece56b90dfae8a8625"
[[qualified_tools.platforms.executables]]
name = "cargo"
path = "bin/cargo"
sha256 = "a670e02cfa39ac2e53f77a2d911ca1fc9e2c479b0603ec956625be1daf24e3af"
probe = { kind = "version", expected = "cargo 1.99.1 (5f94df478 2026-08-27)" }
[[qualified_tools.platforms.executables]]
name = "rustc"
path = "bin/rustc"
sha256 = "b53b780f77f952062f8ca8a3b70aef2390b1e45ad91939be94afdc9543026853"
probe = { kind = "rustc_verbose", expected = "rustc 1.99.1 (58cf8f9c3 2026-10-13)\nbinary: rustc\ncommit-hash: 58cf8f9c35b355199727627d07ae34a4e2846d67\ncommit-date: 2026-10-13\nhost: x86_64-unknown-linux-gnu\nrelease: 1.99.1\nLLVM version: 23.1.1" }
"#;

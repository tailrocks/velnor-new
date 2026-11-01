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
#[path = "operator_toolchain_guard_fixture/data.rs"]
mod data;
#[path = "operator_toolchain_guard_fixture/mise.rs"]
mod mise;

use data::{CARGO_TOML, CONFIG, QUALIFIED_RUST};

pub(super) struct Fixture {
    _temp: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) repo: PathBuf,
    pub(super) workspace: PathBuf,
    pub(super) guard: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        Self::with_repo_dir("repo")
    }

    pub(super) fn with_repo_dir(repo_dir: &str) -> Result<Self, Box<dyn Error>> {
        Self::with_layout(repo_dir, "")
    }

    pub(super) fn with_workspace_dir(workspace_dir: &str) -> Result<Self, Box<dyn Error>> {
        Self::with_layout("repo", workspace_dir)
    }

    fn with_layout(repo_dir: &str, workspace_dir: &str) -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let repo = root.join(repo_dir);
        let workspace = repo.join(workspace_dir);
        let guard = root.join("bin/guard");
        fs::create_dir_all(repo.join(".velnor"))?;
        fs::create_dir_all(root.join("bin"))?;
        fs::create_dir_all(root.join("home"))?;
        fs::create_dir_all(workspace.join("src"))?;
        fs::create_dir_all(repo.join("Formula"))?;
        fs::write(workspace.join("Cargo.toml"), CARGO_TOML)?;
        fs::write(
            workspace.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"probe\"\nversion = \"0.1.0\"\n",
        )?;
        fs::write(workspace.join("src/main.rs"), "fn main() {}\n")?;
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
            format!("{CONFIG}\n{QUALIFIED_RUST}").replace(
                "\"Cargo.toml\"",
                &format!(
                    "\"{}\"",
                    Path::new(workspace_dir).join("Cargo.toml").display()
                ),
            ),
        )?;
        Self::write_metadata(&workspace, &root)?;
        mise::write(&root, &workspace)?;
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
            workspace,
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
        self.invoke_in(&self.repo, args, require_guard)
    }

    pub(super) fn invoke_in(
        &self,
        cwd: &Path,
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
            .current_dir(cwd)
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

    pub(super) fn generate_unguarded_from(
        &self,
        cwd: &Path,
        output: &Path,
    ) -> Result<Output, Box<dyn Error>> {
        self.invoke_in(
            cwd,
            &[
                "generate",
                "--output-dir",
                output.to_str().ok_or("non-UTF-8 output path")?,
            ],
            false,
        )
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

    pub(super) fn fail_qualification(&self, stderr: &str) -> Result<(), Box<dyn Error>> {
        fs::write(self.root.join("bin/qualify-stderr"), stderr)?;
        Ok(())
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

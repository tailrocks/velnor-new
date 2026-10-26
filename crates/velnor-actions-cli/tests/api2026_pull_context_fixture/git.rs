// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, io, process::Command};

use super::{Identities, Paths};

pub(crate) struct Fixture<'a> {
    paths: &'a Paths,
}

impl<'a> Fixture<'a> {
    pub(crate) fn new(paths: &'a Paths) -> Self {
        Self { paths }
    }

    pub(crate) fn initialize(&self, digest: &str) -> Result<Identities, Box<dyn Error>> {
        self.git(&["init", "-b", "main"])?;
        self.git(&["config", "user.name", "Fixture"])?;
        self.git(&["config", "user.email", "fixture@example.invalid"])?;
        self.git(&[
            "remote",
            "add",
            "origin",
            "https://github.com/test-owner/test-tap.git",
        ])?;
        fs::create_dir_all(self.paths.repo.join(".velnor"))?;
        fs::create_dir_all(self.paths.repo.join("src"))?;
        fs::write(self.paths.repo.join("src/lib.rs"), "pub fn fixture() {}\n")?;
        fs::write(self.paths.repo.join(".velnor/config.toml"), CONFIG)?;
        fs::write(
            self.paths.repo.join(".velnor/release-manifest.json"),
            manifest(digest),
        )?;
        self.git(&["add", "."])?;
        self.git(&["commit", "-m", "fixture base"])?;
        let base = self.value(&["rev-parse", "HEAD"])?;
        let root_tree = self.value(&["rev-parse", "HEAD^{tree}"])?;
        let vel_nor_tree = self.value(&["rev-parse", "HEAD:.velnor"])?;
        self.git(&["checkout", "-b", "fixture-head"])?;
        fs::write(self.paths.repo.join("src/head.rs"), "pub fn head() {}\n")?;
        self.git(&["add", "."])?;
        self.git(&["commit", "-m", "fixture head"])?;
        let head = self.value(&["rev-parse", "HEAD"])?;
        let tree = self.value(&["rev-parse", "HEAD^{tree}"])?;
        let merge = self.value(&["commit-tree", &tree, "-p", &base, "-p", &head])?;
        self.git(&["checkout", "--detach", &merge])?;
        Ok(Identities {
            base,
            head,
            merge,
            root_tree,
            vel_nor_tree,
        })
    }

    fn git(&self, args: &[&str]) -> Result<(), Box<dyn Error>> {
        self.call(args).map(|_| ())
    }

    fn value(&self, args: &[&str]) -> Result<String, Box<dyn Error>> {
        self.call(args)
    }

    fn call(&self, args: &[&str]) -> Result<String, Box<dyn Error>> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.paths.repo)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.paths.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.paths.home.join("gitconfig"))
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }
}

fn manifest(digest: &str) -> String {
    let targets = [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ];
    let targets = targets.iter().map(|target| serde_json::json!({
        "target": target,
        "artifact": format!("https://github.com/tailrocks/velnor-new/releases/download/v{}/velnor-actions-{}-{target}", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_VERSION")),
        "sha256": digest
    })).collect::<Vec<_>>();
    serde_json::json!({"schema":1,"version":env!("CARGO_PKG_VERSION"),"repository":"tailrocks/velnor-new","commit":"6930000000000000000000000000000000000000","targets":targets}).to_string()
}

const CONFIG: &str = r#"schema = 2
[workflow]
policy = "consumer-v1"
default_branch = "main"
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
[[native_checks]]
id = "merge-context"
operation = "tap_tree"
timeout_minutes = 1
[native_checks.runner]
label = "ubuntu-26.04"
platform = "linux_x64"
executor = "hosted"
"#;

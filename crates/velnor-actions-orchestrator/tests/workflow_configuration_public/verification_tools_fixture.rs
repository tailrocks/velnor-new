// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use super::super::workflow_configuration_fixture;

pub(super) const LOCK: &str = include_str!("verification_tools.lock");
pub(super) const KINGFISHER: &str = "github:mongodb/kingfisher";
pub(super) const REGEX: &str =
    r"^kingfisher-(darwin|linux)-(arm64|x64)\.tgz$|^kingfisher-windows-x64\.zip$";
pub(super) const RUST: &str = "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ncomponents = [\"clippy\", \"rustfmt\"]\ntargets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"]\n";
pub(super) const TOOLS: [(&str, &str); 7] = [
    ("rust", "1.99.1"),
    ("aqua:nextest-rs/nextest/cargo-nextest", "0.9.149"),
    ("mr-boxington", "1.27.0"),
    ("hk", "2.8.0"),
    ("aqua:gitleaks/gitleaks", "8.30.1"),
    ("aqua:betterleaks/betterleaks", "1.9.0"),
    (KINGFISHER, "2.14.0"),
];

pub(super) fn source(regex: &str) -> String {
    let selector = toml::Value::String(regex.to_owned());
    let tools = TOOLS
        .iter()
        .map(|(key, version)| {
            format!(
                "{} = {}",
                toml::Value::String((*key).to_owned()),
                toml::Value::String((*version).to_owned())
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[tools]\n\"{KINGFISHER}\" = {{ version = \"2.14.0\", matching_regex = {selector} }}\n[tasks.\"verify:hook-closure\"]\nrun = \"printf fixture\\n\"\ntools = {{ {tools} }}\n"
    )
}

pub(super) fn repo(regex: &str, lock: &str) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let config = r#"schema = 2
[workflow]
name = "CI"
policy = "consumer-v1"
default_branch = "testmain"
[[workflow.tasks]]
kind = "verification"
id = "hook-closure"
mise_task = "verify:hook-closure"
runner = "linux-x64"
timeout_minutes = 10
[workflow.tasks.source]
mise_config = ".velnor/hk-check/mise.toml"
working_directory = ".velnor/hk-check"
[execution]
mode = "both"
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
"#;
    let repo = workflow_configuration_fixture::make_repo(config)?;
    workflow_configuration_fixture::git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/fixture/repo.git",
        ],
        repo.path(),
    )?;
    let root = repo.path().join(".velnor/hk-check");
    fs::create_dir_all(&root)?;
    fs::write(root.join("mise.toml"), source(regex))?;
    fs::write(root.join("mise.lock"), lock)?;
    fs::write(root.join("rust-toolchain.toml"), RUST)?;
    fs::write(repo.path().join("rust-toolchain.toml"), RUST)?;
    Ok(repo)
}

pub(super) fn lock_with_regex(regex: &str) -> Result<String, Box<dyn Error>> {
    let mut lock: toml::Value = toml::from_str(LOCK)?;
    lock["tools"][KINGFISHER][0]["options"]["matching_regex"] =
        toml::Value::String(regex.to_owned());
    Ok(toml::to_string(&lock)?)
}

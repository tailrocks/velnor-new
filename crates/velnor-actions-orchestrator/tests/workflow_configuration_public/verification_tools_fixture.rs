// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use super::workflow_configuration_fixture;

pub(super) const LOCK: &str = include_str!("verification_tools.lock");
pub(super) const KINGFISHER: &str = "github:mongodb/kingfisher";
pub(super) const REGEX: &str =
    r"^kingfisher-(darwin|linux)-(arm64|x64)\.tgz$|^kingfisher-windows-x64\.zip$";
pub(super) const RUST: &str = "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ncomponents = [\"clippy\", \"rustfmt\"]\ntargets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"]\n";
pub(super) const TOOLS: [(&str, &str); 7] = [
    ("rust", "1.99.1"),
    ("aqua:nextest-rs/nextest/cargo-nextest", "0.9.151"),
    ("mr-boxington", "1.29.2"),
    ("hk", "2.11.1"),
    ("aqua:gitleaks/gitleaks", "8.30.1"),
    ("aqua:betterleaks/betterleaks", "1.10.0"),
    (KINGFISHER, "2.17.0"),
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
        "[tools]\n\"{KINGFISHER}\" = {{ version = \"2.17.0\", matching_regex = {selector} }}\n[tasks.\"verify:hook-closure\"]\nrun = \"printf fixture\\n\"\ntools = {{ {tools} }}\n"
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

pub(super) fn build_task_repo_with_lock(lock: &str) -> Result<tempfile::TempDir, Box<dyn Error>> {
    const NEXT: &str = "aqua:nextest-rs/nextest/cargo-nextest";
    let config = format!(
        r#"schema = 1

[workflow]
name = "CI"
policy = "consumer-v1"
default_branch = "testmain"

[stacks.rust]
compile_driver = "mbx"

[[workflow.tasks]]
id = "mise-v4-tools"
kind = "build"
mise_task = "verify:hook-closure"
source = {{ mise_config = "mise.toml", working_directory = "." }}
tools = ["{NEXT}", "mr-boxington", "rust"]
runner = "macos-26-arm64"
timeout_minutes = 10
cargo_build_jobs = 1
nextest_test_threads = 1
"#
    );
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    fs::write(
        repo.path().join("mise.toml"),
        format!(
            r#"min_version = "2026.10.18"

[tools]
rust = {{ version = "1.99.1", profile = "minimal", components = ["clippy", "rustfmt"], targets = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"], mr_boxington = true }}
mr-boxington = "1.29.2"
"{NEXT}" = "0.9.151"

[settings]
cargo.binstall = true
idiomatic_version_file_enable_tools = ["rust"]
lockfile = true

[tasks."verify:hook-closure"]
run = "true"
tools = {{ "{NEXT}" = "0.9.151" }}
"#
        ),
    )?;
    fs::write(repo.path().join("mise.lock"), lock)?;
    fs::write(repo.path().join("rust-toolchain.toml"), RUST)?;
    Ok(repo)
}

pub(super) fn build_task_repo_with_source_lock(
    source_lock: &str,
) -> Result<tempfile::TempDir, Box<dyn Error>> {
    const NEXT: &str = "aqua:nextest-rs/nextest/cargo-nextest";
    let repo = build_task_repo_with_lock(LOCK)?;
    let config_path = repo.path().join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?;
    let source = config.replace(
        "source = { mise_config = \"mise.toml\", working_directory = \".\" }",
        "source = { mise_config = \"task/mise.toml\", working_directory = \"task\" }",
    );
    if source == config {
        return Err("missing BuildTask source config line".into());
    }
    fs::write(config_path, source)?;
    let task_root = repo.path().join("task");
    fs::create_dir_all(&task_root)?;
    fs::write(
        task_root.join("mise.toml"),
        format!(
            "[tasks.\"verify:hook-closure\"]\nrun = \"true\"\ntools = {{ \"{NEXT}\" = \"0.9.151\" }}\n"
        ),
    )?;
    fs::write(task_root.join("mise.lock"), source_lock)?;
    fs::copy(
        repo.path().join("rust-toolchain.toml"),
        task_root.join("rust-toolchain.toml"),
    )?;
    Ok(repo)
}

pub(super) fn lock_with_artifact_metadata(
    tool: &str,
    platform: &str,
    provenance_verified: bool,
    owner: &str,
) -> Result<String, Box<dyn Error>> {
    let mut lock: toml::Value = toml::from_str(LOCK)?;
    let artifact = lock["tools"][tool][0][&format!("platforms.{platform}")]
        .as_table_mut()
        .ok_or("missing selected platform artifact")?;
    artifact.insert(
        "provenance_verified".to_owned(),
        toml::Value::Boolean(provenance_verified),
    );
    let repository_ids = artifact
        .entry("repository_ids".to_owned())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or("invalid repository IDs")?;
    repository_ids
        .entry("repository".to_owned())
        .or_insert_with(|| toml::Value::String("1234567890".to_owned()));
    repository_ids.insert("owner".to_owned(), toml::Value::String(owner.to_owned()));
    Ok(toml::to_string(&lock)?)
}

pub(super) fn lock_with_regex(regex: &str) -> Result<String, Box<dyn Error>> {
    let mut lock: toml::Value = toml::from_str(LOCK)?;
    lock["tools"][KINGFISHER][0]["options"]["matching_regex"] =
        toml::Value::String(regex.to_owned());
    Ok(toml::to_string(&lock)?)
}

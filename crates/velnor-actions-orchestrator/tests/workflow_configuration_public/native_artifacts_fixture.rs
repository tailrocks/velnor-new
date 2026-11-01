// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerationPreparation, prepare};

use super::workflow_configuration_fixture;

pub(crate) const TASK_ID: &str = "native-proof";

const MISE_CONFIG: &str = r#"min_version = "2026.10.18"

[tools.rust]
version = "1.99.1"
profile = "minimal"
components = ["clippy", "rustfmt"]
targets = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
mr_boxington = true

[tools.mr-boxington]
version = "1.28.0"

[settings]
lockfile = true
idiomatic_version_file_enable_tools = ["rust"]

[settings.cargo]
binstall = true
binstall_only = true

[tasks.native-artifact-fixture]
run = "true"
"#;

pub(crate) fn build_task_config(artifacts: Option<&str>, suites: &str) -> String {
    let artifacts =
        artifacts.map_or_else(String::new, |body| format!("artifacts = [\n{body}\n]\n"));
    format!(
        r#"schema = 1

[workflow]
name = "CI"
policy = "consumer-v1"
default_branch = "testmain"
generator_validation = "bootstrap"

[stacks.rust]
compile_driver = "mbx"

[discovery]
exclude = []

[[workflow.tasks]]
id = "{TASK_ID}"
kind = "build"
mise_task = "native-artifact-fixture"
source = {{ mise_config = "mise.toml", working_directory = "." }}
tools = ["mr-boxington", "rust"]
runner = "macos-26-arm64"
timeout_minutes = 10
cargo_build_jobs = 1
nextest_test_threads = 1
{artifacts}
{suites}
"#
    )
}

pub(crate) fn build_task_artifact(
    name: &str,
    paths: &[&str],
    when: &str,
    retention_days: u16,
) -> String {
    let paths = paths
        .iter()
        .map(|path| format!("\"{path}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "  {{ name = \"{name}\", paths = [{paths}], when = \"{when}\", retention_days = {retention_days} }},"
    )
}

pub(crate) fn rust_suite_config(compile_driver: &str, partitions: u16, build: &str) -> String {
    format!(
        r#"schema = 1

[workflow]
name = "CI"
policy = "consumer-v1"
default_branch = "testmain"
generator_validation = "bootstrap"

[stacks.rust]
compile_driver = "{compile_driver}"

[[stacks.rust.suites]]
id = "proof-suite"
profile = "default"
triggers = ["pull_request", "push"]
required = true
package = "demo"
manifest_path = "Cargo.toml"
partitions = {partitions}
build = "{build}"
artifacts = [{{ name = "suite-evidence", paths = ["artifacts/suite-evidence.json"], when = "failure", retention_days = 7 }}]
"#
    )
}

pub(crate) fn repo_with_build_task(
    artifacts: Option<&str>,
    suites: &str,
) -> Result<(TempDir, GenerationPreparation), Box<dyn Error>> {
    let config = build_task_config(artifacts, suites);
    repo_with_config_and_build_inputs(&config)
}

pub(crate) fn repo_with_config_and_build_inputs(
    config: &str,
) -> Result<(TempDir, GenerationPreparation), Box<dyn Error>> {
    let repo = repo_with_build_inputs(config)?;
    let prepared = prepare(repo.path())?;
    Ok((repo, prepared))
}

pub(crate) fn repo_with_build_inputs(config: &str) -> Result<TempDir, Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(config)?;
    let root = repo.path();
    fs::write(root.join("mise.toml"), MISE_CONFIG)?;
    fs::write(
        root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ncomponents = [\"clippy\", \"rustfmt\"]\ntargets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"]\n",
    )?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::copy(workspace.join("mise.lock"), root.join("mise.lock"))?;
    Ok(repo)
}

pub(crate) fn repo_with_rust_suite(
    compile_driver: &str,
    partitions: u16,
    build: &str,
) -> Result<(TempDir, GenerationPreparation), Box<dyn Error>> {
    let config = rust_suite_config(compile_driver, partitions, build);
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let prepared = prepare(repo.path())?;
    Ok((repo, prepared))
}

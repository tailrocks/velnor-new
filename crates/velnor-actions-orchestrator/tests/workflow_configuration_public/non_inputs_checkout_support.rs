// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::WorkflowPolicy;

use super::one_job_per_crate::mbx_repo_for;
use super::workflow_configuration_fixture::{self};

pub(super) const STANDALONE_SUITE: &str = "\n[[stacks.rust.suites]]\nid = \"nightly-suite\"\nprofile = \"default\"\ntriggers = [\"workflow_dispatch\"]\npackage = \"demo\"\nmanifest_path = \"crates/demo/Cargo.toml\"\n";

pub(super) const NAMED_CHECK: &str = "\n[[checks]]\nid = \"unit\"\ntask = \"fixture-check\"\ninputs = [\"crates/demo/Cargo.toml\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n";

pub(super) fn rust_repo(
    policy: WorkflowPolicy,
    non_inputs: Option<&str>,
) -> Result<TempDir, Box<dyn Error>> {
    let repo = mbx_repo_for(true, policy)?;
    let root = repo.path();
    fs::remove_dir_all(root.join("src"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/demo\"]\nresolver = \"2\"\n",
    )?;
    fs::create_dir_all(root.join("crates/demo/src"))?;
    fs::write(
        root.join("crates/demo/Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(root.join("crates/demo/src/lib.rs"), "pub fn f() {}\n")?;
    let config_path = repo.path().join(".velnor/config.toml");
    let mut config = fs::read_to_string(&config_path)?;
    config.push_str("\n[stacks.rust]\n");
    if let Some(glob) = non_inputs {
        config.extend(["non_inputs = [\"", glob, "\"]\n"]);
    }
    config.push_str(STANDALONE_SUITE);
    config.push_str(NAMED_CHECK);
    fs::write(
        root.join("mise.toml"),
        "[tasks.fixture-check]\nrun = \"true\"\n",
    )?;
    fs::write(&config_path, config)?;
    fs::create_dir_all(repo.path().join("docs"))?;
    fs::write(repo.path().join("docs/a.md"), "a\n")?;
    Ok(repo)
}

pub(super) fn exclusion(glob: &str) -> String {
    format!("/*\n!/{glob}")
}

pub(super) fn git_output(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = workflow_configuration_fixture::git_fixture::command(root)?
        .args(args)
        .output()?;
    Ok(String::from_utf8(output.stdout)?)
}

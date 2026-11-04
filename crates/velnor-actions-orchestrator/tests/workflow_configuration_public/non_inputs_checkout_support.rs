// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;
use velnor_actions_contract::{Job, Step, StepKind, WorkflowPolicy};
use velnor_actions_orchestrator::{finalized_jobs, prepare};
use velnor_actions_workflow_renderer::non_input_checkout::{
    NON_INPUT_GUARD_STEP, SPARSE_CHECKOUT_KEY,
};

use super::one_job_per_crate::mbx_repo_for;
use super::scoped_workspace::TestResult;
use super::workflow_configuration_fixture::{self, git};

pub(super) const RUST_SUITE: &str = "\n[[stacks.rust.suites]]\nid = \"proof-suite\"\nprofile = \"default\"\ntriggers = [\"pull_request\", \"push\"]\nrequired = true\npackage = \"demo\"\nmanifest_path = \"crates/demo/Cargo.toml\"\n";

pub(super) const STANDALONE_SUITE: &str = "\n[[stacks.rust.suites]]\nid = \"nightly-suite\"\nprofile = \"default\"\ntriggers = [\"workflow_dispatch\"]\npackage = \"demo\"\nmanifest_path = \"crates/demo/Cargo.toml\"\n";

pub(super) const NAMED_CHECK: &str = "\n[[checks]]\nid = \"unit\"\ntask = \"fixture-check\"\ninputs = [\"crates/demo/Cargo.toml\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n";

pub(super) fn checkout_steps(job: &Job) -> Vec<(usize, &Step)> {
    job.steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@"))
        })
        .collect()
}

pub(super) fn with_value<'a>(step: &'a Step, key: &str) -> Option<&'a str> {
    match &step.kind {
        StepKind::Action { with, .. } => with.get(key).map(String::as_str),
        _ => None,
    }
}

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
    config.push_str(RUST_SUITE);
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

fn rendered_rust_job(glob: &str) -> Result<Job, Box<dyn Error>> {
    let repo = rust_repo(WorkflowPolicy::ConsumerV1, Some(glob))?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    jobs.into_iter()
        .find(|(id, _)| id.starts_with("rust-"))
        .map(|(_, job)| job)
        .ok_or_else(|| "no rendered crate job".into())
}

pub(super) fn rendered_patterns(glob: &str) -> Result<String, Box<dyn Error>> {
    let job = rendered_rust_job(glob)?;
    let (_, checkout) = checkout_steps(&job)
        .into_iter()
        .next()
        .ok_or("crate job has no checkout")?;
    Ok(with_value(checkout, SPARSE_CHECKOUT_KEY)
        .ok_or("crate job checkout has no sparse-checkout input")?
        .to_owned())
}

pub(super) fn guard_command(glob: &str, runner_temp: &Path) -> Result<Command, Box<dyn Error>> {
    let job = rendered_rust_job(glob)?;
    let guard = job
        .steps
        .iter()
        .find(|step| step.name == NON_INPUT_GUARD_STEP);
    assert!(
        guard.is_some(),
        "the rendered crate job has no absence guard"
    );
    let StepKind::Shell { run, .. } = &guard.ok_or("guard step missing")?.kind else {
        return Err("guard is not a shell step".into());
    };
    let (program, args) = run.split_first().ok_or("empty guard command")?;
    let mut command = Command::new(program);
    command.args(args).env("RUNNER_TEMP", runner_temp);
    Ok(command)
}

pub(super) fn guard_run(
    glob: &str,
    root: &Path,
    runner_temp: &Path,
) -> Result<Output, Box<dyn Error>> {
    Ok(guard_command(glob, runner_temp)?
        .current_dir(root)
        .output()?)
}

pub(super) fn tracked_tree() -> Result<TempDir, Box<dyn Error>> {
    let tree = tempfile::tempdir()?;
    let root = tree.path();
    git(&["init", "-b", "testmain"], root)?;
    for (path, body) in [
        ("docs/a.md", "a\n"),
        ("docs/guide.md", "g\n"),
        ("docs/sub/b.md", "b\n"),
        ("docsx/c.md", "c\n"),
        ("apps/a1/docs/d.md", "d\n"),
        ("crates/x/src/lib.rs", "pub fn f() {}\n"),
    ] {
        let file = root.join(path);
        fs::create_dir_all(file.parent().ok_or("no parent")?)?;
        fs::write(file, body)?;
    }
    git(&["add", "-A"], root)?;
    git(&["commit", "-m", "tree"], root)?;
    Ok(tree)
}

pub(super) fn git_output(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = workflow_configuration_fixture::git_fixture::command(root)?
        .args(args)
        .output()?;
    Ok(String::from_utf8(output.stdout)?)
}

pub(super) fn apply_exclusion(root: &Path, patterns: &str) -> TestResult {
    git(&["config", "core.sparseCheckout", "true"], root)?;
    let info = git_output(root, &["rev-parse", "--git-path", "info/sparse-checkout"])?;
    fs::create_dir_all(root.join(info.trim()).parent().ok_or("no parent")?)?;
    fs::write(root.join(info.trim()), format!("{patterns}\n"))?;
    git(&["read-tree", "-mu", "HEAD"], root)?;
    Ok(())
}

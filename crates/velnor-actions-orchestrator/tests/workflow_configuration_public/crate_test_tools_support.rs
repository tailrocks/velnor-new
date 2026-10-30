// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use serde::Serialize;
use tempfile::TempDir;
use velnor_actions_contract::cachekey::{SelectedTestTool, ToolchainInputs};
use velnor_actions_contract::{ProposedTask, StepKind};
use velnor_actions_orchestrator::{GenerationPreparation, OrchestratorError, prepare};

use super::workflow_configuration_fixture;

pub(super) const VERSION: &str = "2.8.0";
pub(super) const SIGNER: &str =
    "sigstore-oidc:https://github.com/jdx/hk/.github/workflows/release.yml";

#[derive(Serialize)]
pub(super) struct LegacyToolchainInputs<'a> {
    tools: &'a [String],
    components: &'a [String],
    compile_driver: &'a str,
    test_runner: &'a str,
}

impl<'a> From<&'a ToolchainInputs> for LegacyToolchainInputs<'a> {
    fn from(inputs: &'a ToolchainInputs) -> Self {
        Self {
            tools: &inputs.tools,
            components: &inputs.components,
            compile_driver: &inputs.compile_driver,
            test_runner: &inputs.test_runner,
        }
    }
}

pub(super) fn configured_repo(config: &str, nested_name: &str) -> Result<TempDir, Box<dyn Error>> {
    configured_repo_with_lock(
        config,
        &mise_config(VERSION),
        &valid_lock(VERSION, &"a".repeat(64)),
        nested_name,
    )
}

pub(super) fn configured_repo_with_lock(
    config: &str,
    mise: &str,
    lock: &str,
    nested_name: &str,
) -> Result<TempDir, Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(config)?;
    workflow_configuration_fixture::add_independent_mbx_package(
        repo.path(),
        "crates/nested",
        nested_name,
    )?;
    fs::write(repo.path().join("mise.toml"), mise)?;
    fs::write(repo.path().join("mise.lock"), lock)?;
    Ok(repo)
}

pub(super) fn prepare_with_pin(
    root: &std::path::Path,
    version: &str,
    digest: &str,
) -> Result<GenerationPreparation, Box<dyn Error>> {
    fs::write(root.join("mise.toml"), mise_config(version))?;
    fs::write(root.join("mise.lock"), valid_lock(version, digest))?;
    Ok(prepare(root)?)
}

pub(super) fn config_with_test_tools(entries: &str, runner: Option<&str>) -> String {
    let mut config = workflow_configuration_fixture::config_with_verification_branch(None);
    if let Some(runner) = runner {
        config.push_str("runner_label = \"");
        config.push_str(runner);
        config.push_str("\"\n");
    }
    config.push_str("\n[stacks.rust.test_tools]\n");
    config.push_str(entries);
    config
}

pub(super) fn mise_config(version: &str) -> String {
    format!("[tools]\nhk = \"{version}\"\n")
}

pub(super) fn valid_lock(version: &str, digest: &str) -> String {
    format!(
        "lockfile_version = 3\n\n[[tools.hk]]\nversion = \"{version}\"\nbackend = \"packslip:github.com/jdx/hk\"\nspecifiers = [\"{version}\"]\n\n[tools.hk.\"platforms.linux-x64\"]\nchecksum = \"sha256:{digest}\"\nurl = \"https://github.com/jdx/hk/releases/download/v{version}/hk-x86_64-unknown-linux-gnu.tar.gz\"\nsigner = \"{SIGNER}\"\n"
    )
}

pub(super) fn execution_inputs(
    prepared: &GenerationPreparation,
) -> BTreeMap<String, ToolchainInputs> {
    let mut inputs = BTreeMap::new();
    for job in prepared.workflow.ir.jobs.values() {
        for step in &job.steps {
            if let StepKind::TaskExecution {
                task_id,
                toolchain_inputs,
                ..
            } = &step.kind
            {
                inputs.insert(task_id.clone(), toolchain_inputs.as_ref().clone());
            }
        }
    }
    inputs
}

pub(super) fn task_by_id<'a>(
    prepared: &'a GenerationPreparation,
    task_id: &str,
) -> Result<&'a ProposedTask, Box<dyn Error>> {
    prepared
        .discovery
        .proposals
        .iter()
        .find(|task| task.task_id == task_id)
        .ok_or_else(|| format!("missing task proposal {task_id}").into())
}

pub(super) fn find_task<'a>(
    prepared: &'a GenerationPreparation,
    inputs: &'a BTreeMap<String, ToolchainInputs>,
    package: &str,
    predicate: impl Fn(&ProposedTask) -> bool,
) -> Result<(&'a str, &'a ProposedTask, &'a ToolchainInputs), Box<dyn Error>> {
    for task in &prepared.discovery.proposals {
        if task.display_name == package
            && predicate(task)
            && let Some(toolchain) = inputs.get(&task.task_id)
        {
            return Ok((&task.task_id, task, toolchain));
        }
    }
    Err(format!("missing matching task for {package}").into())
}

pub(super) fn is_test_task(task: &ProposedTask) -> bool {
    matches!(task.task_kind.as_str(), "test" | "nextest" | "doctest") && !task.no_targets
}

pub(super) fn assert_selected_hk(
    tools: &[SelectedTestTool],
    version: &str,
) -> Result<(), Box<dyn Error>> {
    let [tool] = tools else {
        return Err(format!("expected one selected HK tool, got {}", tools.len()).into());
    };
    assert_eq!(tool.version, version);
    assert_eq!(tool.backend, "packslip:github.com/jdx/hk");
    assert_eq!(tool.platform, "linux-x64");
    assert!(tool.options.is_empty());
    assert_eq!(tool.signer, SIGNER);
    let [lock_binding, mise_binding] = tool.source_bindings.as_slice() else {
        return Err(format!(
            "expected two selected source bindings, got {}",
            tool.source_bindings.len()
        )
        .into());
    };
    assert_eq!(lock_binding.path, "mise.lock");
    assert_eq!(mise_binding.path, "mise.toml");
    Ok(())
}

pub(super) fn selected_hk(input: &ToolchainInputs) -> Result<&SelectedTestTool, Box<dyn Error>> {
    input
        .selected_test_tools
        .first()
        .ok_or_else(|| "missing selected HK tool".into())
}

pub(super) fn expect_config_problem(
    result: Result<GenerationPreparation, OrchestratorError>,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    match result {
        Err(OrchestratorError::Config { problem, .. }) if problem == expected => Ok(()),
        Err(error) => Err(format!("expected {expected}, got {error}").into()),
        Ok(_) => Err(format!("accepted invalid selected HK input; expected {expected}").into()),
    }
}

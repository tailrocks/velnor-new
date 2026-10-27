// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error, io};

use velnor_actions_contract::{Job, StepKind, StepRole};
use velnor_actions_orchestrator::GenerationPreparation;
use velnor_actions_workflow_renderer::{FINAL_JOB_ID, PLAN_JOB_ID};

pub(super) fn assert_published_helper_tools(
    preparation: &GenerationPreparation,
    jobs: &BTreeMap<String, Job>,
) -> Result<(), Box<dyn Error>> {
    for key in ["MISE_AUTO_INSTALL", "MISE_EXEC_AUTO_INSTALL"] {
        assert_eq!(
            preparation
                .workflow
                .context
                .plan_consumer_env
                .get(key)
                .map(String::as_str),
            Some("false")
        );
    }
    for (id, expected_gh) in [(PLAN_JOB_ID, "gh@2.103.2"), (FINAL_JOB_ID, "gh@2.103.3")] {
        let job = jobs
            .get(id)
            .ok_or_else(|| io::Error::other(format!("missing helper job {id}")))?;
        let prepare = job
            .steps
            .iter()
            .filter(|step| step.role == Some(StepRole::PreparePinnedTools))
            .collect::<Vec<_>>();
        assert_eq!(prepare.len(), 1, "{id} must have one tool preparation");
        let StepKind::Shell { run, env, .. } = &prepare[0].kind else {
            return Err(io::Error::other(format!("{id} preparation is not native shell")).into());
        };
        assert_eq!(
            run.iter()
                .filter(|argument| *argument == expected_gh)
                .count(),
            1,
            "{id} omitted its authenticated helper GH dependency"
        );
        if id == PLAN_JOB_ID {
            assert!(
                !run.contains(&format!("gh@{}", velnor_actions_mise::catalog::GH_VERSION)),
                "{id} substituted candidate GH for the SDK55 helper GH"
            );
        }
        assert!(
            !run.iter()
                .any(|argument| argument.contains("VELNOR_TOOL_GH")),
            "{id} selected project-controlled helper GH"
        );
        for key in ["MISE_AUTO_INSTALL", "MISE_EXEC_AUTO_INSTALL"] {
            assert!(
                env.get(key).is_none_or(|value| value == "false"),
                "{id} overrides {key}"
            );
        }
    }
    assert_declared_check_mise(jobs)
}

fn assert_declared_check_mise(jobs: &BTreeMap<String, Job>) -> Result<(), Box<dyn Error>> {
    for (id, sha256) in [
        (
            "check-fixture-mise-linux",
            "3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d",
        ),
        (
            "check-fixture-mise-arm",
            "7dc61dfa71abbcda1c40d18440bc556b5d58b80e6042bc4f95fd106fa1bb3c66",
        ),
        (
            "check-fixture-mise-intel",
            "8d5e1591811143f15334f690373cefb58c13bd6cbd0292008748fd777f6c99d8",
        ),
    ] {
        let job = jobs
            .get(id)
            .ok_or_else(|| io::Error::other(format!("missing declared check {id}")))?;
        assert_setup(job, id, "2026.10.19", sha256)?;
    }
    Ok(())
}

pub(super) fn assert_current_mise_jobs(jobs: &BTreeMap<String, Job>) -> Result<(), Box<dyn Error>> {
    let mut ordinary_task = false;
    for (id, job) in jobs {
        if id.starts_with("check-fixture-mise-") {
            continue;
        }
        if !job
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::MiseSetup))
        {
            continue;
        }
        let target = job
            .check_runner
            .as_ref()
            .map(|runner| runner.platform.release_target())
            .or_else(|| velnor_actions_contract::ReleaseTarget::for_runner_label(&job.runs_on))
            .ok_or_else(|| io::Error::other(format!("unsupported fixture target {id}")))?;
        let current = velnor_actions_mise::current_mise_binary(target);
        assert_setup(job, id, velnor_actions_mise::MISE_VERSION, current.sha256)?;
        ordinary_task |= job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::TaskExecution { .. }));
    }
    assert!(
        ordinary_task,
        "fixture omitted ordinary Rust task execution"
    );
    let nextest = format!(
        "aqua:nextest-rs/nextest/cargo-nextest@{}",
        velnor_actions_mise::catalog::NEXTEST_VERSION
    );
    assert!(
        jobs.values()
            .flat_map(|job| &job.steps)
            .any(|step| match &step.kind {
                StepKind::Shell { run, .. } | StepKind::TaskExecution { argv: run, .. } =>
                    run.contains(&nextest),
                _ => false,
            }),
        "ordinary Nextest pin changed with helper runtime"
    );
    Ok(())
}

fn assert_setup(job: &Job, id: &str, version: &str, sha256: &str) -> Result<(), Box<dyn Error>> {
    let setup = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::MiseSetup))
        .collect::<Vec<_>>();
    assert_eq!(setup.len(), 1, "{id} must have exactly one Mise setup");
    let StepKind::Action { with, .. } = &setup[0].kind else {
        return Err(io::Error::other(format!("{id} setup is not an action")).into());
    };
    assert_eq!(
        with.get("version").map(String::as_str),
        Some(version),
        "{id} runtime version"
    );
    assert_eq!(
        with.get("sha256").map(String::as_str),
        Some(sha256),
        "{id} runtime digest"
    );
    Ok(())
}

pub(crate) const BOOTSTRAP_CONFIG: &str = r#"schema = 2
[workflow]
policy = "velnor-repository-v1"
default_branch = "main"
generator_validation = "bootstrap"
[stacks.rust]
test_runner = "cargo_nextest"
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
id = "fixture-native-tree"
operation = "tap_tree"
[native_checks.runner]
label = "ubuntu-26.04"
platform = "linux_x64"
executor = "hosted"
[[checks]]
id = "fixture-mise-linux"
task = "fixture-check"
inputs = ["Cargo.toml"]
tools = []
[checks.runner]
label = "ubuntu-26.04"
platform = "linux_x64"
executor = "hosted"
[[checks]]
id = "fixture-mise-arm"
task = "fixture-check"
inputs = ["Cargo.toml"]
tools = []
[checks.runner]
label = "macos-15"
platform = "macos_arm64"
executor = "hosted"
[[checks]]
id = "fixture-mise-intel"
task = "fixture-check"
inputs = ["Cargo.toml"]
tools = []
[checks.runner]
label = "macos-15-intel"
platform = "macos_x64"
executor = "hosted"
"#;

pub(crate) const CONSUMER_CONFIG: &str = r#"schema = 2
[workflow]
policy = "consumer-v1"
default_branch = "main"
[stacks.rust]
test_runner = "cargo_nextest"
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
"#;

pub(super) fn assert_named_check_runner_rejections(
    preparation: &GenerationPreparation,
) -> Result<(), Box<dyn Error>> {
    let mut changed = preparation.clone();
    let job = changed
        .workflow
        .ir
        .jobs
        .get_mut("check-fixture-mise-arm")
        .ok_or_else(|| io::Error::other("producer omitted MacARM declared check"))?;
    "macos-26".clone_into(&mut job.runs_on);
    let runner = job
        .check_runner
        .as_mut()
        .ok_or_else(|| io::Error::other("declared check omitted typed runner"))?;
    "macos-26".clone_into(&mut runner.label);
    changed.workflow.ir.validate()?;
    let raw_error = velnor_actions_workflow_renderer::render::render_workflow_ir(
        &changed.workflow.ir,
        velnor_actions_contract::WorkflowPolicy::VelnorRepositoryV1,
        changed.workflow.support.as_ref(),
        &changed.workflow.context,
    )
    .err()
    .ok_or_else(|| io::Error::other("raw render accepted changed named-check runner"))?;
    assert!(matches!(raw_error,
        velnor_actions_workflow_renderer::RenderError::InvalidWorkflow(problem)
            if problem == "native_authority_graph"
    ));
    let error = velnor_actions_orchestrator::finalized_jobs(&changed)
        .err()
        .ok_or_else(|| io::Error::other("finalizer accepted changed named-check runner"))?;
    assert!(matches!(error,
        velnor_actions_orchestrator::OrchestratorError::Render { problem }
            if problem == "invalid workflow: native_authority_graph"
    ));
    let path = preparation.root.join(".velnor/config.toml");
    let original = std::fs::read(&path)?;
    let mismatched =
        BOOTSTRAP_CONFIG.replacen("platform = \"macos_arm64\"", "platform = \"linux_x64\"", 1);
    std::fs::write(&path, mismatched)?;
    let result = velnor_actions_orchestrator::prepare(&preparation.root);
    std::fs::write(&path, original)?;
    let error = result
        .err()
        .ok_or_else(|| io::Error::other("prepare accepted mismatched Mac runner platform"))?;
    assert!(matches!(error,
        velnor_actions_orchestrator::OrchestratorError::Config { problem, .. }
            if problem == "hosted_runner_platform_mismatch"
    ));
    Ok(())
}

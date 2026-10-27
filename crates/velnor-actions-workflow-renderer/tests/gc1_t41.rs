// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, NativeHomebrewOperation,
};
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepKind, Trigger, VelnorConfig, WorkflowIr,
    WorkflowPolicy,
};
use velnor_actions_workflow_renderer::render::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, CanonicalAuthoritySteps, CanonicalRuntimeInputs,
    EXPECTED_PR_TYPES, RenderContext, render_workflow_ir,
};
use velnor_actions_workflow_renderer::steps::{REQUEST_DIR_PREFIX, STAGED_BINARY_PREFIX};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn require(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label} lost:{needle}")))
    }
}

fn native_check_step(check_id: &str, task_id: &str) -> Step {
    Step {
        name: "Execute native homebrew check".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::NativeCheckExecution {
            check_id: check_id.to_owned(),
            operation: NativeHomebrewOperation::Install,
            task_id: task_id.to_owned(),
            sdk_version: env!("CARGO_PKG_VERSION").to_owned(),
        },
    }
}

fn native_check_job(step: Step) -> Job {
    Job {
        display_name: "Native check".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: Some(CheckRunner {
            label: "ubuntu-24.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        }),
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        steps: vec![step],
    }
}

fn repository_config() -> Result<VelnorConfig, Box<dyn Error>> {
    toml::from_str(include_str!("../../../.velnor/config.toml"))
        .map_err(|error| fail(&format!("repository config rejected:{error}")))
}

fn plan_env() -> BTreeMap<String, String> {
    velnor_actions_mise::ISOLATION_ENV
        .into_iter()
        .chain(velnor_actions_mise::NO_AUTO_INSTALL_ENV)
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

fn context(jobs: &BTreeMap<String, Job>) -> Result<RenderContext, Box<dyn Error>> {
    let config = repository_config()?;
    let runtime = CanonicalRuntimeInputs::new(
        plan_env(),
        env!("CARGO_PKG_VERSION").to_owned(),
        format!("{STAGED_BINARY_PREFIX}{}", env!("CARGO_PKG_VERSION")),
        format!("{REQUEST_DIR_PREFIX}request"),
        "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683".to_owned(),
        false,
    )
    .map_err(|error| fail(&format!("authority rejected:{error}")))?;
    let authority_steps = CanonicalAuthoritySteps::qualify_jobs(
        jobs,
        WorkflowPolicy::ConsumerV1,
        &config,
        "ubuntu-24.04",
        Vec::new(),
        runtime,
    )
    .map_err(|error| fail(&format!("authority rejected:{error}")))?;
    Ok(RenderContext {
        authority_steps,
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        report_helper_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        scale_set_selector: None,
        staged_binary: format!("{STAGED_BINARY_PREFIX}{}", env!("CARGO_PKG_VERSION")),
        request_dir: format!("{REQUEST_DIR_PREFIX}request"),
        checkout_uses: "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683".to_owned(),
        validator_commands: Vec::new(),
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: config.workflow.pull_request_cache_policy,
        mise_quiet: config.workflow.mise_quiet,
        plan_consumer_env: plan_env(),
        tool_pins: Vec::new(),
    })
}

fn workflow(step: Step) -> WorkflowIr {
    WorkflowIr {
        name: "gc1-t41".to_owned(),
        triggers: Trigger {
            pull_request_types: EXPECTED_PR_TYPES.iter().map(ToString::to_string).collect(),
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs: BTreeMap::from([("check-brew-install".to_owned(), native_check_job(step))]),
    }
}

fn rendered(check_id: &str, task_id: &str) -> Result<String, Box<dyn Error>> {
    let ir = workflow(native_check_step(check_id, task_id));
    match render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &context(&ir.jobs)?) {
        Ok(yaml) => Ok(yaml),
        Err(error) => Err(fail(&format!("render rejected:{error}"))),
    }
}

fn rejected(
    check_id: &str,
    task_id: &str,
    needle: &str,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let ir = workflow(native_check_step(check_id, task_id));
    match render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &context(&ir.jobs)?) {
        Ok(_) => Err(fail(&format!("{label} accepted:{check_id}:{task_id}"))),
        Err(error) => require(&error.to_string(), needle, label),
    }
}

fn check_identity_match() -> Result<(), Box<dyn Error>> {
    let yaml = rendered("brew-install", "internal/native-check/brew-install")?;
    require(
        &yaml,
        "internal/native-check/brew-install",
        "identity match task",
    )?;
    require(&yaml, "VELNOR_TASK_ID", "identity match env")?;
    require(
        &yaml,
        "VELNOR_NATIVE_CHECK_OPERATION",
        "identity match operation",
    )?;
    require(
        &yaml,
        "execute-native-homebrew-check-v1",
        "identity match op",
    )?;
    require(&yaml, "${{ github.token }}", "identity match token")?;
    Ok(())
}

fn check_identity_mismatch() -> Result<(), Box<dyn Error>> {
    rejected(
        "brew-install",
        "internal/native-check/other-check",
        "native_check_token_scope",
        "foreign task",
    )?;
    rejected(
        "other-check",
        "internal/native-check/brew-install",
        "native_check_token_scope",
        "swapped identity",
    )?;
    Ok(())
}

fn check_identity_empty() -> Result<(), Box<dyn Error>> {
    rejected("brew-install", "", "invalid_task_id", "empty task")?;
    rejected(
        "",
        "internal/native-check/brew-install",
        "invalid_check_id",
        "empty check",
    )?;
    Ok(())
}

#[test]
fn gc1_t41_accepts_bound_identity() -> Result<(), Box<dyn Error>> {
    check_identity_match()
}

#[test]
fn gc1_t41_rejects_unbound_identity() -> Result<(), Box<dyn Error>> {
    check_identity_mismatch()?;
    check_identity_empty()
}

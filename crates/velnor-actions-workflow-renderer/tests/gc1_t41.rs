// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, NativeHomebrewOperation,
};
use velnor_actions_contract::workflow::cache_family::{RootSlug, StateUnit};
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepKind, Trigger, VelnorConfig, WorkflowIr,
    WorkflowPolicy,
};
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, state_select_step};
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
        cache_access: velnor_actions_contract::CacheAccess::Read,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: velnor_actions_contract::JobEnv::Unset,
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
        bun_lane_plan: None,
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
        standalone_tools_donors: std::collections::BTreeMap::new(),
        max_parallel_jobs: config.workflow.max_parallel_jobs,
        non_inputs: velnor_actions_contract::NonInputs::default(),
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
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs: BTreeMap::from([("check-brew-install".to_owned(), native_check_job(step))]),
    }
}

fn workflow_with_steps(steps: Vec<Step>) -> WorkflowIr {
    let mut ir = workflow(native_check_step(
        "brew-install",
        "internal/native-check/brew-install",
    ));
    let mut job = super::gc1_workflow_cases_common::job_with(
        "Shell steps",
        velnor_actions_contract::CacheAccess::Read,
        None,
        Vec::new(),
        None,
    );
    job.steps = steps;
    ir.jobs.clear();
    ir.jobs.insert("shells".to_owned(), job);
    ir
}

fn action_mbx_step() -> Result<Step, Box<dyn Error>> {
    let build: Vec<String> = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.91.0",
        "--",
        "mbx",
        "build",
        "--release",
        "--locked",
        "--package",
        "velnor-actions-cli",
        "--bin",
        "velnor-actions",
    ]
    .iter()
    .map(|value| (*value).to_owned())
    .collect();
    Ok(velnor_actions_workflow_renderer::preseed::preseed_build_step(&build, &BTreeMap::new())?)
}

pub(super) fn render(step: Step) -> Result<String, Box<dyn Error>> {
    render_steps(vec![step])
}

fn emitted_selector_run(workflow: &str) -> Result<String, Box<dyn Error>> {
    let step_start = workflow
        .find("name: Select attested state")
        .ok_or_else(|| fail("rendered workflow lost the selector step"))?;
    let line = workflow[step_start..]
        .lines()
        .find(|line| line.trim_start().starts_with("run: "))
        .ok_or_else(|| fail("rendered selector lost its run scalar"))?;
    let value = line
        .trim_start()
        .strip_prefix("run: ")
        .ok_or_else(|| fail("rendered selector run field is malformed"))?;
    let Some(value) = value.strip_prefix('"') else {
        return Ok(value.to_owned());
    };
    let mut decoded = String::new();
    let mut chars = value.chars();
    loop {
        match chars.next() {
            Some('"') => return Ok(decoded),
            Some('\\') => match chars.next() {
                Some('\\') => decoded.push('\\'),
                Some('"') => decoded.push('"'),
                Some('n') => decoded.push('\n'),
                Some('r') => decoded.push('\r'),
                Some('t') => decoded.push('\t'),
                Some(other) => return Err(fail(&format!("unsupported YAML escape:{other}"))),
                None => return Err(fail("rendered selector has an incomplete escape")),
            },
            Some(ch) => decoded.push(ch),
            None => return Err(fail("rendered selector run scalar is unterminated")),
        }
    }
}

#[test]
fn rendered_selector_yaml_keeps_the_final_run_within_21000_utf16_units()
-> Result<(), Box<dyn Error>> {
    let selector = state_select_step(
        &[SealRoot {
            unit: StateUnit::Union,
            slug: RootSlug::from_root(".")?,
        }],
        JobTimeout::PLAN,
    )?;
    let StepKind::Shell { run, env } = &selector.kind else {
        return Err(fail("selector step is not a shell step"));
    };
    let payload = env
        .get("VELNOR_STATE_SELECT_SUPERVISOR_HEX")
        .ok_or_else(|| fail("selector supervisor environment is absent"))?
        .clone();
    let expected_run = velnor_actions_workflow_renderer::join_argv_for_run(run)?;
    let workflow = render(selector)?;
    let rendered_run = emitted_selector_run(&workflow)?;
    if rendered_run.encode_utf16().count() > 21_000 || rendered_run != expected_run {
        return Err(fail("emitted selector YAML run exceeds its final bound"));
    }
    require(
        &workflow,
        &format!("VELNOR_STATE_SELECT_SUPERVISOR_HEX: {payload}"),
        "selector static environment serialization",
    )?;
    Ok(())
}

fn render_steps(steps: Vec<Step>) -> Result<String, Box<dyn Error>> {
    let ir = workflow_with_steps(steps);
    match render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &context(&ir.jobs)?) {
        Ok(yaml) => Ok(yaml),
        Err(error) => Err(fail(&format!("render rejected:{error}"))),
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
fn gc1_t41_rejects_truncated_action_mbx_exec_argv() -> Result<(), Box<dyn Error>> {
    let canonical = action_mbx_step()?;
    render_steps(vec![canonical.clone()])?;

    let error = velnor_actions_workflow_renderer::steps::shell_step(
        "Reject bare exec",
        vec!["exec".to_owned()],
        BTreeMap::new(),
    )
    .expect_err("bare exec must be rejected");
    require(
        &error.to_string(),
        "exec_missing_command",
        "bare exec rejection",
    )?;

    for shell in ["sh", "bash", "/bin/sh"] {
        let mut malformed = canonical.clone();
        let StepKind::Shell { run, .. } = &mut malformed.kind else {
            return Err(fail("action-owned MBX build was not a shell step"));
        };
        *run = vec!["exec".to_owned(), shell.to_owned(), "-c".to_owned()];
        let ir = workflow_with_steps(vec![malformed]);
        let error = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &context(&ir.jobs)?)
            .expect_err("truncated action-owned MBX argv must be rejected");
        if !error
            .to_string()
            .contains("action_owned_mbx_binding:not_inline_shell")
        {
            return Err(fail(&format!("unexpected malformed argv error:{error}")));
        }
    }

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

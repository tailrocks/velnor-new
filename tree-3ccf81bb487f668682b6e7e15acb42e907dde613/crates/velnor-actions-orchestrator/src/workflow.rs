//! Workflow-IR, render-context, and actionlint-input construction.

use std::collections::BTreeMap;

use velnor_actions_actionlint::{ActionlintConfigInput, IgnorePolicy};
use velnor_actions_contract::{
    Concurrency, GeneratorValidation, Job, Permissions, Step, StepKind, Trigger, VelnorConfig,
    VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::{TaskGroup, TaskKind};
use velnor_actions_workflow_renderer::render::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, FINAL_CONDITION, FINAL_DISPLAY_NAME,
    FINAL_JOB_ID, PLAN_JOB_ID, PolicyCommand, RenderContext, TASK_JOB_ID, WORKFLOW_PATH,
};
use velnor_actions_workflow_renderer::steps::{
    REQUEST_DIR_PREFIX, STAGED_BINARY_PREFIX, merge_step, plan_step,
};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::vectors::{candidate_spec, task_argv, verify_tools_argv};

/// Pinned `actions/checkout` ref (cli-contract section 4 sample pin).
pub const CHECKOUT_USES: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

/// Default literal runner label when the config omits the override.
pub const DEFAULT_RUNNER_LABEL: &str = "ubuntu-26.04";

/// Fixed request directory rendered for internal plan/merge steps.
pub(crate) const REQUEST_DIR: &str = "$RUNNER_TEMP/velnor/request";

/// Complete renderer input derived from one discovery.
#[derive(Debug, Clone)]
pub struct WorkflowPlan {
    /// Stack-neutral workflow IR.
    pub ir: WorkflowIr,
    /// Support jobs for the Velnor policy, none for consumers.
    pub support: Option<VelnorSupportWorkflow>,
    /// Validated renderer scalars.
    pub context: RenderContext,
    /// Actionlint config input.
    pub actionlint: ActionlintConfigInput,
}

/// Build renderer input from config, branch, label, and discovery.
///
/// # Errors
///
/// Returns contract, render-context, or tool-request errors.
pub(crate) fn build_workflow(
    config: &VelnorConfig,
    branch: &str,
    label: &str,
    discovery: &Discovery,
) -> Result<WorkflowPlan, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    let version = env!("CARGO_PKG_VERSION").to_owned();
    let policy = config.workflow.policy;
    let support = match policy {
        WorkflowPolicy::ConsumerV1 => None,
        WorkflowPolicy::VelnorRepositoryV1 => {
            Some(policy.support_workflow(config.workflow.generator_validation))
        }
    };
    let mut jobs = BTreeMap::new();
    jobs.insert(PLAN_JOB_ID.to_owned(), plan_job(label));
    let task_groups: Vec<&TaskGroup> = discovery
        .task_groups
        .iter()
        .filter(|group| !group.no_test_targets)
        .collect();
    if !task_groups.is_empty() {
        jobs.insert(
            TASK_JOB_ID.to_owned(),
            task_job(label, &task_groups, &catalog)?,
        );
    }
    jobs.insert(
        FINAL_JOB_ID.to_owned(),
        final_job(label, !task_groups.is_empty()),
    );
    let ir = WorkflowIr {
        name: config.workflow.name.clone(),
        triggers: Trigger {
            pull_request_types: EXPECTED_PR_TYPES.iter().map(ToString::to_string).collect(),
            push_branches: vec![branch.to_owned()],
            merge_group: true,
        },
        permissions: Permissions {
            contents: "read".to_owned(),
            actions: "read".to_owned(),
        },
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
    };
    let context = render_context(config, label, &version, &catalog)?;
    let actionlint = actionlint_input(policy, &version);
    Ok(WorkflowPlan {
        ir,
        support,
        context,
        actionlint,
    })
}

/// Planner job: checkout plus the fixed plan step.
fn plan_job(label: &str) -> Job {
    Job {
        display_name: "Velnor Plan".to_owned(),
        runs_on: label.to_owned(),
        needs: Vec::new(),
        condition: None,
        steps: vec![checkout_action(), plan_step()],
    }
}

/// Matrix consumer job: checkout plus one fixed vector per runnable group.
fn task_job(
    label: &str,
    groups: &[&TaskGroup],
    catalog: &ToolCatalog,
) -> Result<Job, OrchestratorError> {
    let mut steps = Vec::with_capacity(groups.len() + 1);
    steps.push(checkout_action());
    for group in groups {
        steps.push(task_step(group, catalog)?);
    }
    Ok(Job {
        display_name: "Velnor Task".to_owned(),
        runs_on: label.to_owned(),
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        steps,
    })
}

/// Final gate with the exact required-check name and `always()` condition.
fn final_job(label: &str, with_task: bool) -> Job {
    let mut needs = vec![PLAN_JOB_ID.to_owned()];
    if with_task {
        needs.push(TASK_JOB_ID.to_owned());
    }
    Job {
        display_name: FINAL_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        needs,
        condition: Some(FINAL_CONDITION.to_owned()),
        steps: vec![merge_step()],
    }
}

/// Pinned checkout action without persisted credentials.
fn checkout_action() -> Step {
    let mut with = BTreeMap::new();
    with.insert("persist-credentials".to_owned(), "false".to_owned());
    Step {
        name: "Checkout".to_owned(),
        kind: StepKind::Action {
            uses: CHECKOUT_USES.to_owned(),
            with,
        },
    }
}

/// One fixed-vector step for a runnable task group.
fn task_step(group: &TaskGroup, catalog: &ToolCatalog) -> Result<Step, OrchestratorError> {
    let argv = task_argv(group, catalog)?;
    Ok(Step {
        name: task_step_name(group),
        kind: StepKind::Shell {
            run: argv,
            env: BTreeMap::new(),
        },
    })
}

/// Display name derived from group kind, package, and configuration.
fn task_step_name(group: &TaskGroup) -> String {
    let kind = match group.kind {
        TaskKind::Fmt => "Format",
        TaskKind::Clippy => "Clippy",
        TaskKind::Test => "Test",
        TaskKind::Nextest => "Nextest",
        TaskKind::Doctest => "Doctests",
        TaskKind::Doc => "Doc",
        TaskKind::Build => "Build",
    };
    let what = if group.package_name.is_empty() {
        "workspace".to_owned()
    } else {
        group.package_name.clone()
    };
    format!("{kind} {what} ({})", group.configuration)
}

/// Renderer scalars: version, label, staged path, request dir, pins.
fn render_context(
    config: &VelnorConfig,
    label: &str,
    version: &str,
    catalog: &ToolCatalog,
) -> Result<RenderContext, OrchestratorError> {
    debug_assert!(REQUEST_DIR.starts_with(REQUEST_DIR_PREFIX));
    let velnor = config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1;
    let policy_commands = if velnor {
        vec![PolicyCommand {
            name: "Verify pinned tools".to_owned(),
            argv: verify_tools_argv(catalog)?,
        }]
    } else {
        Vec::new()
    };
    let candidate =
        if velnor && config.workflow.generator_validation == GeneratorValidation::Candidate {
            Some(candidate_spec(catalog)?)
        } else {
            None
        };
    Ok(RenderContext {
        generator_version: version.to_owned(),
        runs_on: label.to_owned(),
        staged_binary: format!("{STAGED_BINARY_PREFIX}{version}"),
        request_dir: REQUEST_DIR.to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        policy_commands,
        candidate,
    })
}

/// Actionlint input: generated workflow path plus policy-graded ignores.
fn actionlint_input(policy: WorkflowPolicy, version: &str) -> ActionlintConfigInput {
    let mut input = ActionlintConfigInput::new(version).with_workflow_path(WORKFLOW_PATH);
    input.policy = match policy {
        WorkflowPolicy::ConsumerV1 => IgnorePolicy::Consumer,
        WorkflowPolicy::VelnorRepositoryV1 => IgnorePolicy::VelnorProtected,
    };
    input
}

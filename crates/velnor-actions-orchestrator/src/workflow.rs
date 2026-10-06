//! Workflow-IR, render-context, and actionlint-input construction.

//! W1 emission wiring lives in the child module below.
#[path = "wire_w1.rs"]
pub(crate) mod wire_w1;
#[path = "workflow_context.rs"]
mod workflow_context;
#[path = "workflow_dispatch.rs"]
mod workflow_dispatch;

#[path = "check_jobs.rs"]
pub(crate) mod check_jobs;

use std::collections::BTreeMap;

use velnor_actions_actionlint::{ActionlintConfigInput, IgnorePolicy, StepSyntax};
use velnor_actions_contract::{
    Concurrency, GeneratorValidation, Job, Permissions, Stack, Step, StepKind, StepRole, Trigger,
    ValidatorKind, VelnorConfig, VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_mise::{
    PREPARE_RUST_COMPONENTS_STEP, PrepareRustComponents, ToolCatalog, ToolHomes,
};
use velnor_actions_rust::TestRunner;
use velnor_actions_workflow_renderer::render::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, FINAL_JOB_ID, PLAN_JOB_ID,
    PUBLISH_JOB_ID, RenderContext, WORKFLOW_PATH,
};
use velnor_actions_workflow_renderer::steps::PLAN_OPERATION;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::pins::consumer_acquire_step;
use crate::utf8::{strings_of, strings_of_env};
use crate::workflow_jobs::{PlanJobToolNeeds, PlanRustNeed, final_job, lint_job, plan_job};

pub(crate) use crate::workflow_jobs::LINT_JOB_ID;

/// Pinned `actions/checkout` ref (cli-contract section 4 sample pin).
pub const CHECKOUT_USES: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

/// Default literal runner label when the config omits the override.
pub const DEFAULT_RUNNER_LABEL: &str = "ubuntu-26.04";

/// Fixed request directory rendered for internal plan/merge steps.
///
/// GitHub-expression spelling: shell `$VAR` never expands in the `env:`
/// position that carries this path.
pub(crate) const REQUEST_DIR: &str = "${{ runner.temp }}/velnor/request";

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

/// Build the plan job with its deferred format steps attached.
///
/// # Errors
///
/// Returns tool-request or step-construction errors.
fn build_plan_job(
    label: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    needs: PlanJobToolNeeds,
    fetch_roots: &[String],
    discovery: &Discovery,
) -> Result<Job, OrchestratorError> {
    let mut plan = plan_job(label, acquire, catalog, needs, fetch_roots)?;
    insert_format_report_steps(
        &mut plan,
        wire_w1::workspace_format_report_steps(discovery)?,
    );
    Ok(plan)
}

/// Build renderer input from config, branch, label, and discovery.
///
/// # Errors
///
/// Returns contract, render-context, or tool-request errors.
pub(crate) fn build_workflow(
    root: &std::path::Path,
    config: &VelnorConfig,
    branch: &str,
    label: &str,
    discovery: &Discovery,
    fetch_roots: &[String],
) -> Result<WorkflowPlan, OrchestratorError> {
    wire_w1::vet_step_syntax(StepSyntax::JobMatrix)?;
    let catalog = ToolCatalog::pinned();
    let version = env!("CARGO_PKG_VERSION").to_owned();
    let policy = config.workflow.policy;
    let workflow_tasks = crate::workflow_task_jobs::policies(root, config, discovery)?;
    let support = support_workflow(policy, config.workflow.generator_validation, discovery);
    let mut jobs = BTreeMap::new();
    let acquire = match policy {
        WorkflowPolicy::ConsumerV1 => Some(consumer_acquire_step(label, &version, discovery)?),
        WorkflowPolicy::VelnorRepositoryV1 => None,
    };
    let format = wire_w1::workspace_format_step(discovery, &catalog)?;
    let rust = match (plan_uses_rust(discovery, policy), format.is_some()) {
        (false, false) => PlanRustNeed::None,
        (_, true) => PlanRustNeed::CompilerAndComponents,
        (true, false) => PlanRustNeed::Compiler,
    };
    let needs = PlanJobToolNeeds {
        rust,
        nextest: plan_uses_nextest(discovery),
        opentofu: plan_uses_opentofu(discovery),
        gh: policy == WorkflowPolicy::VelnorRepositoryV1,
    };
    let mut plan = build_plan_job(
        label,
        acquire.clone(),
        &catalog,
        needs,
        fetch_roots,
        discovery,
    )?;
    if let Some(format) = format {
        insert_format_step(&mut plan, format);
    }
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        plan.permissions = Some(crate::workflow_jobs::read_actions_permissions());
    }
    jobs.insert(PLAN_JOB_ID.to_owned(), plan);
    let built = crate::crate_jobs::build_for_workflow(
        config,
        label,
        discovery,
        &catalog,
        fetch_roots,
        acquire.as_ref(),
    )?;
    let mut required_ids: Vec<String> = built.jobs.iter().map(|(id, _)| id.clone()).collect();
    for (id, job) in built.jobs {
        jobs.insert(id, job);
    }
    for (id, job) in check_jobs::build_check_jobs(policy, discovery, &catalog)? {
        required_ids.push(id.clone());
        jobs.insert(id, job);
    }
    crate::workflow_task_jobs::insert_jobs(&mut jobs, &workflow_tasks)?;
    insert_gate_jobs(&mut jobs, label, branch, &required_ids, acquire, &catalog)?;
    wire_w1::check_crate_mbx_gating(&jobs, &built.drivers)?;
    let ir = workflow_ir(config, branch, jobs, policy);
    let context = workflow_context::render_context(
        config,
        label,
        &version,
        &catalog,
        discovery,
        rust.has_compiler(),
        workflow_tasks,
    )?;
    let actionlint = actionlint_input(config, &version, label);
    Ok(WorkflowPlan {
        ir,
        support,
        context,
        actionlint,
    })
}

fn workflow_ir(
    config: &VelnorConfig,
    branch: &str,
    jobs: BTreeMap<String, Job>,
    policy: WorkflowPolicy,
) -> WorkflowIr {
    WorkflowIr {
        name: config.workflow.name.clone(),
        triggers: Trigger {
            pull_request_types: EXPECTED_PR_TYPES.iter().map(ToString::to_string).collect(),
            push_branches: vec![branch.to_owned()],
            merge_group: true,
            workflow_dispatch: (policy == WorkflowPolicy::VelnorRepositoryV1)
                .then(workflow_dispatch::qualification_dispatch),
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
    }
}

fn support_workflow(
    policy: WorkflowPolicy,
    validation: GeneratorValidation,
    discovery: &Discovery,
) -> Option<VelnorSupportWorkflow> {
    let mut support = match policy {
        WorkflowPolicy::ConsumerV1 => return None,
        WorkflowPolicy::VelnorRepositoryV1 => policy.support_workflow(validation),
    };
    if discovery.workspaces.is_empty() {
        support
            .validators
            .retain(|validator| *validator != ValidatorKind::CargoDeny);
    }
    Some(support)
}

/// Insert the lint, final-gate, and baseline-publish jobs.
///
/// The publish job closes the graph after the final gate: it needs
/// Required, so it runs only when the gate passed.
///
/// # Errors
///
/// Returns contract, render-context, or tool-request errors.
fn insert_gate_jobs(
    jobs: &mut BTreeMap<String, Job>,
    label: &str,
    branch: &str,
    crate_ids: &[String],
    acquire: Option<Step>,
    catalog: &ToolCatalog,
) -> Result<(), OrchestratorError> {
    jobs.insert(LINT_JOB_ID.to_owned(), lint_job(label, catalog)?);
    jobs.insert(
        FINAL_JOB_ID.to_owned(),
        final_job(label, crate_ids, acquire.clone(), catalog)?,
    );
    jobs.insert(
        PUBLISH_JOB_ID.to_owned(),
        crate::publish_job::baseline_publish_job(label, branch, acquire)?,
    );
    Ok(())
}

/// Insert deferred format-report steps immediately after `Plan`.
///
/// The plan exists only after the planner runs; the publish closure
/// later lands between `Plan` and these steps, so the plan artifact
/// carries plan and matrix only, never the format entry's reports.
/// Without a `plan-v1` anchor the steps close the job.
fn insert_format_report_steps(plan: &mut Job, reports: Vec<Step>) {
    if reports.is_empty() {
        return;
    }
    let at = plan
        .steps
        .iter()
        .position(|step| {
            matches!(&step.kind, StepKind::Internal { operation, .. } if operation == PLAN_OPERATION)
        })
        .map_or(plan.steps.len(), |plan_at| plan_at + 1);
    plan.steps.splice(at..at, reports);
}

/// Insert the workspace `Format` step immediately before `Plan`.
///
/// Mirrors the renderer plan-format anchoring at this stage: the
/// freshness and publish closures do not exist yet, so the `plan-v1`
/// step is the anchor; without it the step closes the job.
fn insert_format_step(plan: &mut Job, format: Step) {
    let at = plan
        .steps
        .iter()
        .position(|step| {
            matches!(&step.kind, StepKind::Internal { operation, .. } if operation == PLAN_OPERATION)
        })
        .unwrap_or(plan.steps.len());
    plan.steps.insert(at, format);
}

/// True when any selected workspace runs tests through Nextest; only those
/// legs resolve the pinned runner.
fn plan_uses_nextest(discovery: &Discovery) -> bool {
    discovery
        .workspaces
        .iter()
        .any(|workspace| workspace.profile.test_runner == TestRunner::CargoNextest)
}

/// True when any proposal runs through the pinned Opentofu driver.
///
/// Tofu has no workspace profiles, so the plan derives its tofu role
/// from task proposals (the same per-task signal crate jobs group
/// on), never from workspace scans.
pub(crate) fn plan_uses_opentofu(discovery: &Discovery) -> bool {
    discovery
        .proposals
        .iter()
        .any(|task| Stack::from_id(&task.stack_id) == Some(Stack::Tofu))
}

/// True when the plan job needs the Rust toolchain.
///
/// Consumers require Rust for selected Rust evidence (selected or ignored
/// Rust projects, or Rust task proposals without inventory records).
/// Velnor also builds its candidate-source helper in Plan.
pub(crate) fn plan_uses_rust(discovery: &Discovery, policy: WorkflowPolicy) -> bool {
    policy == WorkflowPolicy::VelnorRepositoryV1
        || !discovery.workspaces.is_empty()
        || discovery.statuses.iter().any(|status| {
            let project = match status {
                velnor_actions_contract::DetectionStatus::Selected(project)
                | velnor_actions_contract::DetectionStatus::Ignored { project, .. } => project,
            };
            Stack::from_id(&project.stack_id) == Some(Stack::Rust)
        })
        || discovery
            .proposals
            .iter()
            .any(|task| Stack::from_id(&task.stack_id) == Some(Stack::Rust))
}

/// Typed `Prepare Rust components` step, shared by plan and task jobs.
///
/// Runs second, right after `Prepare pinned tools`: the pinned toolchain
/// exists by then, so the fixed `rustup component add` guarantees
/// clippy/rustfmt idempotently under the owned homes.
///
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
pub(crate) fn prepare_rust_components_step(
    catalog: &ToolCatalog,
) -> Result<Step, OrchestratorError> {
    let request = PrepareRustComponents::new(ToolHomes::runner_temp());
    let run = strings_of(request.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = strings_of_env(&request.env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let mut step = velnor_actions_workflow_renderer::ambient_shell_step(
        PREPARE_RUST_COMPONENTS_STEP,
        run,
        env,
    )
    .map_err(OrchestratorError::from)?;
    step.role = Some(StepRole::PrepareRustComponents);
    Ok(step)
}

/// Actionlint input: generated workflow path plus policy-graded ignores.
///
/// The bridge label is the effective configured runner label (F2):
/// the emitted `self-hosted-runner` entry must match `runs-on`, never
/// a hardcoded distro.
fn actionlint_input(config: &VelnorConfig, version: &str, label: &str) -> ActionlintConfigInput {
    let policy = config.workflow.policy;
    let mut input = ActionlintConfigInput::new(version)
        .with_workflow_path(WORKFLOW_PATH)
        .with_config_variables(wire_w1::declared_config_variables())
        .with_runner_label(label);
    if let Some(execution) = &config.execution {
        input.extra_runner_labels = execution.actionlint_labels();
    }
    input.policy = match policy {
        WorkflowPolicy::ConsumerV1 => IgnorePolicy::Consumer,
        WorkflowPolicy::VelnorRepositoryV1 => IgnorePolicy::VelnorProtected,
    };
    input
}

//! Workflow-IR, render-context, and actionlint-input construction.

//! W1 emission wiring lives in the child module below (self-declared via
//! `#[path]` so `lib.rs` stays untouched); the integrator only registers
//! the companion test file.
#[path = "wire_w1.rs"]
pub(crate) mod wire_w1;

use std::collections::BTreeMap;

use velnor_actions_actionlint::{ActionlintConfigInput, IgnorePolicy, StepSyntax};
use velnor_actions_contract::{
    Concurrency, GeneratorValidation, Job, Permissions, Stack, Step, StepKind, Trigger,
    ValidatorKind, VelnorConfig, VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_mise::{
    PREPARE_RUST_COMPONENTS_STEP, PrepareRustComponents, ToolCatalog, ToolHomes,
};
use velnor_actions_rust::{CompileDriver, TestRunner};
use velnor_actions_workflow_renderer::render::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, FINAL_JOB_ID, PLAN_JOB_ID,
    PUBLISH_JOB_ID, RenderContext, ValidatorCommand, WORKFLOW_PATH,
};
use velnor_actions_workflow_renderer::steps::{
    DENY_STEP_NAME, MACHETE_STEP_NAME, PLAN_OPERATION, REQUEST_DIR_PREFIX, STAGED_BINARY_PREFIX,
};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::pins::consumer_acquire_step;
use crate::utf8::{strings_of, strings_of_env};
use crate::vectors::{ZIZMOR_STEP_NAME, candidate_spec, deny_argv, machete_argv, zizmor_argv};
use crate::workflow_jobs::{final_job, lint_job, plan_job};

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
#[expect(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "one call site threads job scope plus role selection"
)]
fn build_plan_job(
    label: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
    fetch_roots: &[String],
    discovery: &Discovery,
) -> Result<Job, OrchestratorError> {
    let mut plan = plan_job(
        label,
        acquire,
        catalog,
        use_rust,
        use_mbx,
        use_nextest,
        use_opentofu,
        fetch_roots,
    )?;
    if let Some(format) = wire_w1::workspace_format_step(discovery, catalog)? {
        insert_format_step(&mut plan, format);
    }
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
    let use_mbx = plan_uses_mbx(discovery);
    let verify = crate::verify::verify_kinds(&config.workflow.verify.jobs)?;
    let support = crate::verify::build_support(config, &verify);
    let mut jobs = BTreeMap::new();
    let acquire = match policy {
        WorkflowPolicy::ConsumerV1 => Some(consumer_acquire_step(label, &version, discovery)?),
        WorkflowPolicy::VelnorRepositoryV1 => None,
    };
    let use_nextest = plan_uses_nextest(discovery);
    let use_opentofu = plan_uses_opentofu(discovery);
    let use_rust = plan_uses_rust(discovery);
    let plan = build_plan_job(
        label,
        acquire.clone(),
        &catalog,
        use_rust,
        use_mbx,
        use_nextest,
        use_opentofu,
        fetch_roots,
        discovery,
    )?;
    jobs.insert(PLAN_JOB_ID.to_owned(), plan);
    let custom_tasks: &[String] = config
        .stacks
        .rust
        .as_ref()
        .map_or(&[], |rust| &rust.custom_tasks);
    let built = crate::crate_jobs::build_crate_jobs(
        label,
        policy,
        discovery,
        &catalog,
        fetch_roots,
        custom_tasks,
        acquire.as_ref(),
        config.workflow.max_parallel_jobs,
    )?;
    let crate_ids: Vec<String> = built.jobs.iter().map(|(id, _)| id.clone()).collect();
    for (id, job) in built.jobs {
        jobs.insert(id, job);
    }
    insert_gate_jobs(&mut jobs, label, branch, &crate_ids, acquire, &catalog)?;
    wire_w1::check_crate_mbx_gating(&jobs, &built.drivers)?;
    let ir = WorkflowIr {
        name: config.workflow.name.clone(),
        triggers: Trigger {
            pull_request_types: EXPECTED_PR_TYPES.iter().map(ToString::to_string).collect(),
            push_branches: vec![branch.to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
    };
    let context = render_context(config, label, &version, &catalog, use_rust, &verify)?;
    let actionlint = actionlint_input(config, &version, label);
    Ok(WorkflowPlan {
        ir,
        support,
        context,
        actionlint,
    })
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
            matches!(&step.kind, StepKind::Internal { operation } if operation == PLAN_OPERATION)
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
            matches!(&step.kind, StepKind::Internal { operation } if operation == PLAN_OPERATION)
        })
        .unwrap_or(plan.steps.len());
    plan.steps.insert(at, format);
}

/// True when any selected workspace compiles through MBX.
///
/// The plan job pre-installs the MBX driver only on detected project
/// evidence, never by default; consumers without MBX stay Cargo-only.
pub(crate) fn plan_uses_mbx(discovery: &Discovery) -> bool {
    discovery
        .workspaces
        .iter()
        .any(|workspace| workspace.profile.compile_driver == CompileDriver::Mbx)
}

/// True when any selected workspace runs tests through Nextest.
///
/// Both prepare steps union the Nextest runner on this signal, so
/// `cargo_nextest` legs resolve the pinned runner while `cargo_test`
/// legs never carry it.
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
/// Every repo keeps it except pure-tofu ones: tofu work with zero
/// rust workspaces. Repos with neither keep it too (fail-safe: an
/// unneeded install costs seconds, a missing toolchain fails the
/// format/build steps).
fn plan_uses_rust(discovery: &Discovery) -> bool {
    !(plan_uses_opentofu(discovery) && discovery.workspaces.is_empty())
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
    velnor_actions_workflow_renderer::ambient_shell_step(PREPARE_RUST_COMPONENTS_STEP, run, env)
        .map_err(OrchestratorError::from)
}

/// Renderer scalars: version, label, staged path, request dir, pins.
///
/// The plan-consumer env follows the plan role: pure-tofu plans run
/// the plan-op and freshness steps triple-less, every other role
/// keeps the owned-homes triple.
fn render_context(
    config: &VelnorConfig,
    label: &str,
    version: &str,
    catalog: &ToolCatalog,
    plan_needs_rust: bool,
    verify: &[ValidatorKind],
) -> Result<RenderContext, OrchestratorError> {
    debug_assert!(REQUEST_DIR.starts_with(REQUEST_DIR_PREFIX));
    let velnor = config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1;
    let mut validator_commands = if velnor {
        vec![
            ValidatorCommand {
                validator: ValidatorKind::CargoDeny,
                name: DENY_STEP_NAME.to_owned(),
                argv: deny_argv()?,
            },
            ValidatorCommand {
                validator: ValidatorKind::CargoMachete,
                name: MACHETE_STEP_NAME.to_owned(),
                argv: machete_argv()?,
            },
            ValidatorCommand {
                validator: ValidatorKind::Zizmor,
                name: ZIZMOR_STEP_NAME.to_owned(),
                argv: zizmor_argv(catalog)?,
            },
        ]
    } else {
        Vec::new()
    };
    crate::verify::push_verify_commands(&mut validator_commands, verify, catalog)?;
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
        validator_commands,
        candidate,
        preseed: false,
        plan_consumer_env: crate::matrix_step::task_step_env(
            catalog,
            &std::collections::BTreeMap::new(),
            plan_needs_rust,
        )?,
    })
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

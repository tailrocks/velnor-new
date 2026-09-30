//! Workflow-IR, render-context, and actionlint-input construction.

//! W1 emission wiring lives in the child module below (self-declared via
//! `#[path]` so `lib.rs` stays untouched); the integrator only registers
//! the companion test file.
#[path = "wire_w1.rs"]
pub(crate) mod wire_w1;

use std::collections::BTreeMap;

use velnor_actions_actionlint::{ActionlintConfigInput, IgnorePolicy, StepSyntax};
use velnor_actions_contract::{
    Concurrency, GeneratorValidation, Job, Permissions, Step, StepKind, Trigger, ValidatorKind,
    VelnorConfig, VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_mise::{
    PREPARE_RUST_COMPONENTS_STEP, PrepareRustComponents, ToolCatalog, ToolHomes,
};
use velnor_actions_rust::{CompileDriver, TestRunner};
use velnor_actions_workflow_renderer::render::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, FINAL_JOB_ID, PLAN_JOB_ID,
    RenderContext, ValidatorCommand, WORKFLOW_PATH,
};
use velnor_actions_workflow_renderer::steps::{
    DENY_STEP_NAME, MACHETE_STEP_NAME, PLAN_OPERATION, REQUEST_DIR_PREFIX, STAGED_BINARY_PREFIX,
};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::pins::{
    MISSING_MANIFEST_PROBLEM, RELEASE_MANIFEST_ENV, consumer_acquire_step, release_manifest_json,
};
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
    let support = match policy {
        WorkflowPolicy::ConsumerV1 => None,
        WorkflowPolicy::VelnorRepositoryV1 => {
            Some(policy.support_workflow(config.workflow.generator_validation))
        }
    };
    let mut jobs = BTreeMap::new();
    let acquire = match policy {
        WorkflowPolicy::ConsumerV1 => Some(consumer_acquire_step(label, &version, discovery)?),
        WorkflowPolicy::VelnorRepositoryV1 => None,
    };
    let use_nextest = plan_uses_nextest(discovery);
    let mut plan = plan_job(
        label,
        acquire.clone(),
        &catalog,
        use_mbx,
        use_nextest,
        fetch_roots,
    )?;
    if let Some(format) = wire_w1::workspace_format_step(discovery, &catalog)? {
        insert_format_step(&mut plan, format);
    }
    jobs.insert(PLAN_JOB_ID.to_owned(), plan);
    let built = crate::crate_jobs::build_crate_jobs(label, discovery, &catalog, fetch_roots)?;
    let crate_ids: Vec<String> = built.jobs.iter().map(|(id, _)| id.clone()).collect();
    for (id, job) in built.jobs {
        jobs.insert(id, job);
    }
    jobs.insert(LINT_JOB_ID.to_owned(), lint_job(label, &catalog)?);
    jobs.insert(
        FINAL_JOB_ID.to_owned(),
        final_job(label, &crate_ids, acquire, &catalog)?,
    );
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
    let consumer_manifest = match policy {
        WorkflowPolicy::ConsumerV1 => release_manifest_json(discovery),
        WorkflowPolicy::VelnorRepositoryV1 => None,
    };
    let context = render_context(
        config,
        label,
        &version,
        &catalog,
        consumer_manifest.as_deref(),
    )?;
    let actionlint = actionlint_input(policy, &version);
    Ok(WorkflowPlan {
        ir,
        support,
        context,
        actionlint,
    })
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
    Ok(Step {
        name: PREPARE_RUST_COMPONENTS_STEP.to_owned(),
        kind: StepKind::Shell { run, env },
    })
}

/// Renderer scalars: version, label, staged path, request dir, pins.
///
/// Consumer builds carry the resolved release manifest in the shared
/// Check/Plan env so the staged binary regenerates without a bake; the
/// acquire gate above already failed when no source held a manifest, so
/// `None` here is unreachable and fails closed with the same diagnostic.
/// Velnor builds never carry it: lock/preseed paths never call the
/// consumer gate and dogfood goldens must not churn.
fn render_context(
    config: &VelnorConfig,
    label: &str,
    version: &str,
    catalog: &ToolCatalog,
    consumer_manifest: Option<&str>,
) -> Result<RenderContext, OrchestratorError> {
    debug_assert!(REQUEST_DIR.starts_with(REQUEST_DIR_PREFIX));
    let velnor = config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1;
    let validator_commands = if velnor {
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
    let candidate =
        if velnor && config.workflow.generator_validation == GeneratorValidation::Candidate {
            Some(candidate_spec(catalog)?)
        } else {
            None
        };
    let mut extra = BTreeMap::new();
    if !velnor {
        let Some(manifest) = consumer_manifest else {
            return Err(OrchestratorError::Contract {
                problem: MISSING_MANIFEST_PROBLEM.to_owned(),
            });
        };
        extra.insert(RELEASE_MANIFEST_ENV.to_owned(), manifest.to_owned());
    }
    Ok(RenderContext {
        generator_version: version.to_owned(),
        runs_on: label.to_owned(),
        staged_binary: format!("{STAGED_BINARY_PREFIX}{version}"),
        request_dir: REQUEST_DIR.to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        validator_commands,
        candidate,
        preseed: false,
        plan_consumer_env: crate::matrix_step::task_step_env(catalog, &extra)?,
    })
}

/// Actionlint input: generated workflow path plus policy-graded ignores.
fn actionlint_input(policy: WorkflowPolicy, version: &str) -> ActionlintConfigInput {
    let mut input = ActionlintConfigInput::new(version)
        .with_workflow_path(WORKFLOW_PATH)
        .with_config_variables(wire_w1::declared_config_variables());
    input.policy = match policy {
        WorkflowPolicy::ConsumerV1 => IgnorePolicy::Consumer,
        WorkflowPolicy::VelnorRepositoryV1 => IgnorePolicy::VelnorProtected,
    };
    input
}

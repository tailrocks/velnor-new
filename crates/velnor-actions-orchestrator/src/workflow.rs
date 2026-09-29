//! Workflow-IR, render-context, and actionlint-input construction.

//! W1 emission wiring lives in the child module below (self-declared via
//! `#[path]` so `lib.rs` stays untouched); the integrator only registers
//! the companion test file.
#[path = "wire_w1.rs"]
pub(crate) mod wire_w1;

use std::collections::BTreeMap;

use velnor_actions_actionlint::{ActionlintConfigInput, IgnorePolicy, StepSyntax};
use velnor_actions_contract::{
    Concurrency, GeneratorValidation, Permissions, Step, StepKind, Trigger, VelnorConfig,
    VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_mise::{
    PREPARE_RUST_COMPONENTS_STEP, PrepareRustComponents, ToolCatalog, ToolHomes,
};
use velnor_actions_rust::{CompileDriver, TaskGroup, TestRunner};
use velnor_actions_workflow_renderer::render::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, FINAL_JOB_ID, PLAN_JOB_ID,
    PolicyCommand, RenderContext, TASK_JOB_ID, WORKFLOW_PATH,
};
use velnor_actions_workflow_renderer::steps::{
    DENY_STEP_NAME, MACHETE_STEP_NAME, REQUEST_DIR_PREFIX, STAGED_BINARY_PREFIX,
};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::pins::consumer_acquire_step;
use crate::utf8::{strings_of, strings_of_env};
use crate::vectors::{
    ZIZMOR_STEP_NAME, candidate_spec, deny_argv, machete_argv, verify_tools_argv, zizmor_argv,
};
use crate::workflow_jobs::{final_job, lint_job, plan_job, task_job};

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
    jobs.insert(
        PLAN_JOB_ID.to_owned(),
        plan_job(
            label,
            acquire.clone(),
            &catalog,
            use_mbx,
            use_nextest,
            fetch_roots,
        )?,
    );
    let task_groups: Vec<&TaskGroup> = discovery
        .task_groups
        .iter()
        .filter(|group| !group.no_test_targets)
        .collect();
    if !task_groups.is_empty() {
        jobs.insert(
            TASK_JOB_ID.to_owned(),
            task_job(
                label,
                config.workflow.max_parallel_jobs,
                &catalog,
                use_mbx,
                use_nextest,
                fetch_roots,
            )?,
        );
    }
    jobs.insert(LINT_JOB_ID.to_owned(), lint_job(label, &catalog)?);
    jobs.insert(
        FINAL_JOB_ID.to_owned(),
        final_job(label, !task_groups.is_empty(), acquire, &catalog)?,
    );
    wire_w1::ensure_plan_format_step(&mut jobs, discovery, &catalog)?;
    wire_w1::check_task_mbx_gating(&jobs, !task_groups.is_empty(), use_mbx)?;
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

/// True when any selected workspace compiles through MBX.
///
/// The plan job pre-installs the MBX driver only on detected project
/// evidence, never by default; consumers without MBX stay Cargo-only.
fn plan_uses_mbx(discovery: &Discovery) -> bool {
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
fn render_context(
    config: &VelnorConfig,
    label: &str,
    version: &str,
    catalog: &ToolCatalog,
) -> Result<RenderContext, OrchestratorError> {
    debug_assert!(REQUEST_DIR.starts_with(REQUEST_DIR_PREFIX));
    let velnor = config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1;
    let policy_commands = if velnor {
        vec![
            PolicyCommand {
                name: "Verify pinned tools".to_owned(),
                argv: verify_tools_argv(catalog)?,
            },
            PolicyCommand {
                name: DENY_STEP_NAME.to_owned(),
                argv: deny_argv()?,
            },
            PolicyCommand {
                name: MACHETE_STEP_NAME.to_owned(),
                argv: machete_argv()?,
            },
            PolicyCommand {
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
    Ok(RenderContext {
        generator_version: version.to_owned(),
        runs_on: label.to_owned(),
        staged_binary: format!("{STAGED_BINARY_PREFIX}{version}"),
        request_dir: REQUEST_DIR.to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        policy_commands,
        candidate,
        preseed: false,
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

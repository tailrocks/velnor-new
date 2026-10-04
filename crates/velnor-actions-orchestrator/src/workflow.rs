//! Workflow-IR, render-context, and actionlint-input construction.

//! W1 emission wiring lives in the child module below.
#[path = "wire_w1.rs"]
pub(crate) mod wire_w1;
#[path = "workflow_context.rs"]
mod workflow_context;
pub(crate) use workflow_context::collect_rust_source_helpers;
#[path = "workflow_build_jobs.rs"]
mod workflow_build_jobs;
#[path = "workflow_verification.rs"]
mod workflow_verification;

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_actionlint::{ActionlintConfigInput, StepSyntax};
use velnor_actions_contract::{
    Concurrency, Job, Permissions, Stack, Step, StepKind, Trigger, VelnorConfig,
    VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_mise::{
    PREPARE_RUST_COMPONENTS_STEP, PrepareRustComponents, ToolCatalog, ToolHomes,
};
use velnor_actions_rust::{CompileDriver, TestRunner};
use velnor_actions_workflow_renderer::render::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, FINAL_JOB_ID, PUBLISH_JOB_ID,
    RenderContext,
};
use velnor_actions_workflow_renderer::steps::PLAN_OPERATION;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::utf8::{strings_of, strings_of_env};
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
    /// Detached cache producer proposals for review; never live job publication.
    pub receipt_drafts: Vec<crate::cache_producer_workflow::DraftCacheProducerWorkflow>,
    /// Detached MBX domains and explicit cold outcomes; grants no runtime access.
    pub(crate) mbx_finalization: crate::workflow_mbx_finalize::MbxFinalization,
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
    insert_format_report_steps(
        &mut plan,
        wire_w1::workspace_format_report_steps(discovery)?,
    );
    if let Some(format) = wire_w1::workspace_format_step(discovery, catalog, label)? {
        insert_format_step(&mut plan, format);
    }
    Ok(plan)
}

/// Build renderer input from the prepared repository root and discovery.
///
/// # Errors
///
/// Returns contract, render-context, or tool-request errors.
pub(crate) fn build_workflow(
    root: &Path,
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
    let built = workflow_build_jobs::build_jobs(&workflow_build_jobs::BuildScope {
        root,
        config,
        branch,
        label,
        discovery,
        fetch_roots,
        catalog: &catalog,
        version: &version,
    })?;
    let mut context = built.context;
    let verification = config.workflow.verification.as_ref();
    let ir = WorkflowIr {
        cache_mode: velnor_actions_contract::CacheMode::Read,
        run_name: None,
        name: config.workflow.name.clone(),
        triggers: Trigger {
            pull_request_types: EXPECTED_PR_TYPES.iter().map(ToString::to_string).collect(),
            push_tags: Vec::new(),
            push_branches: vec![branch.to_owned()],
            merge_group: true,
            workflow_dispatch: verification
                .and_then(workflow_verification::workflow_dispatch_trigger),
            schedule: verification.and_then(workflow_verification::schedule_trigger),
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs: built.jobs,
    };
    context.source_helpers = collect_rust_source_helpers(&ir, &version)?;
    context
        .source_helpers
        .extend(crate::tofu_cached_init::collect_records(&ir, &version)?);
    context.source_helpers.extend(built.helpers);
    let actionlint = workflow_context::actionlint_input(policy, &version, label);
    Ok(WorkflowPlan {
        ir,
        support: None,
        context,
        actionlint,
        receipt_drafts: built.receipt_drafts,
        mbx_finalization: built.mbx_finalization,
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
        crate::publish_job::baseline_publish_job(label, branch, acquire, catalog)?,
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

/// Insert the workspace `Format` obligation immediately after `Plan`.
///
/// The completed plan supplies the exact task coverage gate and report identity.
fn insert_format_step(plan: &mut Job, format: Step) {
    let at = plan
        .steps
        .iter()
        .position(|step| {
            matches!(&step.kind, StepKind::Internal { operation } if operation == PLAN_OPERATION)
        })
        .map_or(plan.steps.len(), |at| at + 1);
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
/// Consumers require Rust only for selected Rust evidence. The generator
/// repository additionally builds its candidate-source helper in Plan.
fn plan_uses_rust(discovery: &Discovery, policy: WorkflowPolicy) -> bool {
    policy == WorkflowPolicy::VelnorRepositoryV1
        || !discovery.workspaces.is_empty()
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
    let run = strings_of(request.argv(catalog)?)
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = strings_of_env(&request.env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    velnor_actions_workflow_renderer::ambient_shell_step(PREPARE_RUST_COMPONENTS_STEP, run, env)
        .map_err(OrchestratorError::from)
}

#[cfg(test)]
#[path = "workflow_tools_tests.rs"]
mod workflow_tools_tests;

#[cfg(test)]
#[path = "workflow_source_integration_tests.rs"]
mod workflow_source_integration_tests;

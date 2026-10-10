//! Workflow-IR, render-context, and actionlint-input construction.

//! W1 emission wiring lives in the child module below.
#[path = "wire_w1.rs"]
pub(crate) mod wire_w1;
#[path = "workflow_build.rs"]
mod workflow_build;
#[path = "workflow_context.rs"]
mod workflow_context;
pub(super) use workflow_build::{WorkflowBuildInput, build_workflow_for_consumer_release};
#[path = "workflow_dispatch.rs"]
mod workflow_dispatch;

#[path = "check_jobs.rs"]
pub(crate) mod check_jobs;

use std::collections::BTreeMap;

use velnor_actions_actionlint::{ActionlintConfigInput, IgnorePolicy};
use velnor_actions_contract::{
    Concurrency, GeneratorValidation, Job, Permissions, Step, StepKind, Trigger, ValidatorKind,
    VelnorConfig, VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, FINAL_JOB_ID, PUBLISH_JOB_ID,
    RenderContext, WORKFLOW_PATH,
};
use velnor_actions_workflow_renderer::steps::PLAN_OPERATION;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::workflow_jobs::{PlanJobToolNeeds, final_job, lint_job, plan_job};

#[path = "workflow_policy.rs"]
pub(super) mod workflow_policy;
pub(crate) use workflow_policy::prepare_rust_components_step;

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
    /// Support jobs: policy validators plus config-selected verification.
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
#[cfg(test)]
pub(super) fn build_workflow(
    root: &std::path::Path,
    config: &VelnorConfig,
    branch: &str,
    label: &str,
    discovery: &Discovery,
    fetch_roots: &[String],
) -> Result<WorkflowPlan, OrchestratorError> {
    build_workflow_for_consumer_release(&WorkflowBuildInput {
        root,
        config,
        branch,
        label,
        discovery,
        fetch_roots,
        consumer_release_version: env!("CARGO_PKG_VERSION"),
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
    verify: &[ValidatorKind],
) -> Option<VelnorSupportWorkflow> {
    let mut support = match policy {
        WorkflowPolicy::ConsumerV1 => {
            if verify.is_empty() {
                return None;
            }
            VelnorSupportWorkflow {
                validators: verify.to_vec(),
                candidate_validation: false,
            }
        }
        WorkflowPolicy::VelnorRepositoryV1 => policy.support_workflow(validation),
    };
    if discovery.workspaces.is_empty() {
        support
            .validators
            .retain(|validator| *validator != ValidatorKind::CargoDeny);
    }
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        for kind in verify {
            if !support.validators.contains(kind) {
                support.validators.push(*kind);
            }
        }
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

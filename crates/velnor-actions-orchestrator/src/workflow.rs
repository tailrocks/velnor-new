#[path = "wire_w1.rs"]
pub(crate) mod wire_w1;
#[path = "workflow_build.rs"]
mod workflow_build;
#[path = "workflow_context.rs"]
mod workflow_context;
pub(super) use workflow_build::{WorkflowBuildInput, build_workflow_for_consumer_release};
#[path = "workflow_dispatch.rs"]
mod workflow_dispatch;

#[path = "verifier_actions.rs"]
pub(crate) mod verifier_actions;

#[path = "check_jobs.rs"]
pub(crate) mod check_jobs;

use std::collections::BTreeMap;

use velnor_actions_actionlint::JactionlintConfigInput;
use velnor_actions_contract::{
    Concurrency, ExecutionMode, GeneratorValidation, Job, Permissions, ScaleSetSelector, Step,
    StepKind, Trigger, ValidatorKind, VelnorConfig, VelnorSupportWorkflow, WorkflowIr,
    WorkflowPolicy,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, FINAL_JOB_ID, RenderContext,
};
use velnor_actions_workflow_renderer::steps::PLAN_OPERATION;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::workflow_jobs::{final_job, lint_job};

#[path = "workflow_policy.rs"]
pub(super) mod workflow_policy;
pub(crate) use workflow_policy::jactionlint_verify_step::{
    VERIFY_JACTIONLINT_STEP, argv as jactionlint_verify_argv, verify_jactionlint_step,
};
pub(crate) use workflow_policy::prepare_rust_components_step;
pub(crate) use workflow_policy::rust_manifest_step::{
    NOT_CATALOGUED_CODE, ToolchainPhase, VERIFY_RESTORED_RUST_STEP, VERIFY_RUST_MANIFEST_STEP,
    toolchain_script, verify_restored_rust_step, verify_rust_argv, verify_rust_manifest_step,
    verify_rust_owned_argv,
};

pub(crate) use crate::workflow_jobs::LINT_JOB_ID;

pub(crate) fn scale_set_selector_for_dispatch(
    config: &VelnorConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<Option<ScaleSetSelector>, OrchestratorError> {
    workflow_context::scale_set_selector(config, dispatch)
}

pub const CHECKOUT_USES: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

pub const DEFAULT_RUNNER_LABEL: &str = "ubuntu-26.04";

pub(crate) const REQUEST_DIR: &str = "${{ runner.temp }}/velnor/request";

#[derive(Debug, Clone)]
pub struct WorkflowPlan {
    pub ir: WorkflowIr,
    pub support: Option<VelnorSupportWorkflow>,
    pub context: RenderContext,
    pub lint_config: JactionlintConfigInput,
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
        cache_mode: None,
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

fn insert_gate_jobs(
    jobs: &mut BTreeMap<String, Job>,
    label: &str,
    crate_ids: &[String],
    acquire: Option<Step>,
    catalog: &ToolCatalog,
) -> Result<(), OrchestratorError> {
    jobs.insert(LINT_JOB_ID.to_owned(), lint_job(label, catalog)?);
    jobs.insert(
        FINAL_JOB_ID.to_owned(),
        final_job(label, crate_ids, acquire)?,
    );
    Ok(())
}

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

fn insert_format_step(plan: &mut Job, format: Step, after_plan: bool) {
    let plan_at = plan
        .steps
        .iter()
        .position(|step| {
            matches!(&step.kind, StepKind::Internal { operation, .. } if operation == PLAN_OPERATION)
        })
        .unwrap_or(plan.steps.len());
    let at = plan_at + usize::from(after_plan && plan_at < plan.steps.len());
    plan.steps.insert(at, format);
}

pub(super) fn lint_config_input(config: &VelnorConfig, version: &str) -> JactionlintConfigInput {
    JactionlintConfigInput::new(version)
        .with_profile(config.workflow.lint.profile)
        .with_config_variables(wire_w1::declared_config_variables(config))
}

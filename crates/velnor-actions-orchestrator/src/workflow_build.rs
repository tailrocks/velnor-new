//! Ordered phases for constructing one workflow plan.

use std::collections::BTreeMap;

use velnor_actions_actionlint::StepSyntax;
use velnor_actions_contract::{
    Job, Step, ValidatorKind, VelnorConfig, VelnorSupportWorkflow, WorkflowPolicy,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::pins::consumer_acquire_step;
use crate::workflow_jobs::{PlanJobToolNeeds, PlanRustNeed};

use super::workflow_policy::{plan_uses_nextest, plan_uses_opentofu, plan_uses_rust};
use super::{
    WorkflowPlan, actionlint_input, build_plan_job, check_jobs, insert_format_step,
    insert_gate_jobs, support_workflow, wire_w1, workflow_context, workflow_ir,
};

/// Inputs used to prepare the workflow's plan, task, and support jobs.
pub(crate) struct WorkflowBuildInput<'a> {
    pub(crate) root: &'a std::path::Path,
    pub(crate) config: &'a VelnorConfig,
    pub(crate) branch: &'a str,
    pub(crate) label: &'a str,
    pub(crate) discovery: &'a Discovery,
    pub(crate) fetch_roots: &'a [String],
    pub(crate) consumer_release_version: &'a str,
    /// Repo-resolved catalog shared with the rest of the preparation;
    /// workflow building never re-resolves or falls back to compile pins.
    pub(crate) catalog: &'a ToolCatalog,
}

struct PreparedWorkflow {
    catalog: ToolCatalog,
    report_helper_version: String,
    generator_version: &'static str,
    policy: WorkflowPolicy,
    workflow_tasks: Vec<WorkflowTaskPolicy>,
    verify: Vec<ValidatorKind>,
    support: Option<VelnorSupportWorkflow>,
    acquire: Option<Step>,
    format: Option<Step>,
    needs: PlanJobToolNeeds,
}

pub(crate) fn build_workflow_for_consumer_release(
    input: &WorkflowBuildInput<'_>,
) -> Result<WorkflowPlan, OrchestratorError> {
    let mut prepared = prepare_workflow(input)?;
    let plan = build_workflow_plan(input, &mut prepared)?;
    let jobs = build_workflow_jobs(input, &mut prepared, plan)?;
    let mut ir = workflow_ir(input.config, input.branch, jobs, prepared.policy);
    let context = workflow_context::render_context(workflow_context::RenderContextInputs {
        config: input.config,
        label: input.label,
        generator_version: prepared.generator_version,
        report_helper_version: &prepared.report_helper_version,
        catalog: &prepared.catalog,
        discovery: input.discovery,
        plan_needs_rust: prepared.needs.rust.has_compiler(),
        workflow_tasks: prepared.workflow_tasks,
        verify: &prepared.verify,
    })?;
    if input.config.agents.claude_md == velnor_actions_contract::ClaudeMdMode::Symlink
        && let Some(plan_job) = ir.jobs.get_mut(PLAN_JOB_ID)
        && let Some(at) = plan_job.steps.iter().position(|step| {
            matches!(
                &step.kind,
                velnor_actions_contract::StepKind::Internal { operation, .. }
                    if operation == velnor_actions_workflow_renderer::steps::PLAN_OPERATION
            )
        })
    {
        let check = velnor_actions_workflow_renderer::closure::freshness_step_with_claude_mode(
            &context.staged_binary,
            velnor_actions_workflow_renderer::closure::FRESHNESS_OUTDIR,
            &context.plan_consumer_env,
            input.config.agents.claude_md,
        )?;
        plan_job.steps.insert(at, check);
    }
    let actionlint = actionlint_input(input.config, prepared.generator_version, input.label);
    Ok(WorkflowPlan {
        ir,
        support: prepared.support,
        context,
        actionlint,
    })
}

fn prepare_workflow(input: &WorkflowBuildInput<'_>) -> Result<PreparedWorkflow, OrchestratorError> {
    wire_w1::vet_step_syntax(StepSyntax::JobMatrix)?;
    wire_w1::vet_step_syntax(StepSyntax::NativeParallelism)?;
    crate::rust_suites_emit::validate_suites_against_repo(
        input.root,
        input.config,
        input.discovery,
    )?;
    let catalog = input.catalog;
    let report_helper_version = input.consumer_release_version.to_owned();
    let generator_version = env!("CARGO_PKG_VERSION");
    let policy = input.config.workflow.policy;
    let workflow_tasks =
        crate::workflow_task_jobs::policies(input.root, input.config, input.discovery)?;
    let validation = input.config.workflow.generator_validation;
    let verify = crate::verify::verify_kinds(&input.config.workflow.verify.jobs)?;
    let support = support_workflow(policy, validation, input.discovery, &verify);
    let acquire = match policy {
        WorkflowPolicy::ConsumerV1 => Some(consumer_acquire_step(
            input.label,
            &report_helper_version,
            input.discovery,
        )?),
        WorkflowPolicy::VelnorRepositoryV1 => None,
    };
    let format = wire_w1::workspace_format_step(input.discovery, catalog)?;
    let rust = match (plan_uses_rust(input.discovery, policy), format.is_some()) {
        (false, false) => PlanRustNeed::None,
        (_, true) => PlanRustNeed::CompilerAndComponents,
        (true, false) => PlanRustNeed::Compiler,
    };
    let needs = PlanJobToolNeeds {
        rust,
        nextest: plan_uses_nextest(input.discovery),
        opentofu: plan_uses_opentofu(input.discovery),
        gh: policy == WorkflowPolicy::VelnorRepositoryV1,
    };
    Ok(PreparedWorkflow {
        catalog: catalog.clone(),
        report_helper_version,
        generator_version,
        policy,
        workflow_tasks,
        verify,
        support,
        acquire,
        format,
        needs,
    })
}

fn build_workflow_plan(
    input: &WorkflowBuildInput<'_>,
    prepared: &mut PreparedWorkflow,
) -> Result<Job, OrchestratorError> {
    let mut plan = build_plan_job(
        input.label,
        prepared.acquire.clone(),
        &prepared.catalog,
        prepared.needs,
        input.fetch_roots,
        input.discovery,
    )?;
    if let Some(format) = prepared.format.take() {
        insert_format_step(&mut plan, format);
    }
    if prepared.policy == WorkflowPolicy::VelnorRepositoryV1 {
        plan.permissions = Some(crate::workflow_jobs::read_actions_permissions());
    }
    Ok(plan)
}

fn build_workflow_jobs(
    input: &WorkflowBuildInput<'_>,
    prepared: &mut PreparedWorkflow,
    plan: Job,
) -> Result<BTreeMap<String, Job>, OrchestratorError> {
    let mut jobs = BTreeMap::new();
    jobs.insert(PLAN_JOB_ID.to_owned(), plan);
    let built = crate::crate_jobs::build_for_workflow(
        input.config,
        input.label,
        input.discovery,
        &prepared.catalog,
        input.fetch_roots,
        prepared.acquire.as_ref(),
        &prepared.report_helper_version,
    )?;
    let mut required_ids: Vec<String> = built.jobs.iter().map(|(id, _)| id.clone()).collect();
    for (id, job) in built.jobs {
        jobs.insert(id, job);
    }
    for (id, job) in
        check_jobs::build_check_jobs(prepared.policy, input.discovery, &prepared.catalog)?
    {
        required_ids.push(id.clone());
        jobs.insert(id, job);
    }
    let (suite_jobs, suite_required_ids) = crate::rust_suites_emit::build_ci_jobs(
        input.root,
        input.config,
        input.label,
        input.discovery,
        &prepared.catalog,
    )?;
    for (id, job) in suite_jobs {
        jobs.insert(id, job);
    }
    required_ids.extend(suite_required_ids);
    crate::workflow_task_jobs::insert_jobs(&mut jobs, &prepared.workflow_tasks)?;
    insert_gate_jobs(
        &mut jobs,
        input.label,
        &required_ids,
        prepared.acquire.take(),
        &prepared.catalog,
    )?;
    wire_w1::check_crate_mbx_gating(&jobs, &built.drivers)?;
    Ok(jobs)
}

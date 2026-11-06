// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_actionlint::StepSyntax;
use velnor_actions_contract::GeneratorLock;
use velnor_actions_contract::{
    Job, Step, ValidatorKind, VelnorConfig, VelnorSupportWorkflow, WorkflowPolicy,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::pins::consumer_acquire_step;
use crate::workflow_jobs::{PlanJobToolNeeds, PlanRustNeed, plan_job};

#[path = "workflow_native_authority.rs"]
mod native_authority;

use super::workflow_policy::{
    plan_uses_all_features, plan_uses_nextest, plan_uses_opentofu, plan_uses_rust,
};
use super::{
    WorkflowPlan, check_jobs, insert_format_step, insert_gate_jobs, lint_config_input,
    support_workflow, wire_w1, workflow_context, workflow_ir,
};

pub(crate) struct WorkflowBuildInput<'a> {
    pub(crate) root: &'a std::path::Path,
    pub(crate) config: &'a VelnorConfig,
    pub(crate) branch: &'a str,
    pub(crate) label: &'a str,
    pub(crate) discovery: &'a Discovery,
    pub(crate) fetch_roots: &'a [String],
    pub(crate) consumer_release_version: &'a str,
    pub(crate) published_generator_lock: Option<&'a GeneratorLock>,
    pub(crate) approved_release:
        Option<velnor_actions_mise::catalog::lock::ApprovedGeneratorRelease<'a>>,
    pub(crate) catalog: &'a ToolCatalog,
}

struct PreparedWorkflow {
    private: Option<crate::config::private_wiring::PrivateWiring>,
    catalog: ToolCatalog,
    helper_catalog: ToolCatalog,
    report_helper_version: String,
    generator_version: &'static str,
    policy: WorkflowPolicy,
    workflow_tasks: Vec<WorkflowTaskPolicy>,
    verify: Vec<ValidatorKind>,
    support: Option<VelnorSupportWorkflow>,
    acquire: Option<Step>,
    format: Option<Step>,
    needs: PlanJobToolNeeds,
    state_recipes: BTreeMap<String, velnor_actions_contract::workflow::cache_family::S256>,
}

pub(crate) fn build_workflow_for_consumer_release(
    input: &WorkflowBuildInput<'_>,
) -> Result<WorkflowPlan, OrchestratorError> {
    let mut prepared = prepare_workflow(input)?;
    let plan = build_workflow_plan(input, &mut prepared)?;
    let mut jobs = build_workflow_jobs(input, &mut prepared, plan)?;
    if input.config.workflow.checkout_lfs {
        wire_w1::apply_checkout_lfs(&mut jobs);
    }
    let mut ir = workflow_ir(input.config, input.branch, jobs, prepared.policy);
    check_jobs::require_history_push_branches(input.discovery, &ir.triggers.push_branches)?;
    let native = native_authority::build(input, &prepared, &mut ir.jobs)?;
    let runtime = native.runtime();
    let authority_steps = native.authority();
    let context = workflow_context::render_context(workflow_context::RenderContextInputs {
        authority_steps,
        runtime,
        config: input.config,
        label: input.label,
        generator_version: prepared.generator_version,
        catalog: &prepared.catalog,
        discovery: input.discovery,
        workflow_tasks: prepared.workflow_tasks,
        verify: &prepared.verify,
    })?;
    if prepared.policy != WorkflowPolicy::VelnorRepositoryV1
        && input.config.agents.claude_md == velnor_actions_contract::ClaudeMdMode::Symlink
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
            context.controller.path(),
            velnor_actions_workflow_renderer::closure::FRESHNESS_OUTDIR,
            &context.plan_consumer_env,
            input.config.agents.claude_md,
        )?;
        plan_job.steps.insert(at, check);
    }
    let lint_config = lint_config_input(input.config, prepared.generator_version);
    Ok(WorkflowPlan {
        ir,
        support: prepared.support,
        context,
        lint_config,
    })
}

fn lane_workflow_tasks(
    input: &WorkflowBuildInput<'_>,
    acquire: Option<&Step>,
    private: Option<&crate::config::private_wiring::PrivateWiring>,
) -> Result<
    Vec<velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy>,
    OrchestratorError,
> {
    let lane = crate::matrix_step::LaneTools::union(
        input.config.workflow.policy,
        input.catalog,
        &crate::crate_job_ids::group_runnable(&input.discovery.proposals)?,
    );
    crate::workflow_task_jobs::policies(
        input.root,
        input.config,
        &crate::image_build_tasks::PrebuiltInputs {
            discovery: input.discovery,
            catalog: input.catalog,
            lane: &lane,
            fetch_roots: input.fetch_roots,
            acquire,
            private,
        },
    )
}

fn controller_acquisition(
    input: &WorkflowBuildInput<'_>,
    policy: WorkflowPolicy,
) -> Result<(String, Option<Step>), OrchestratorError> {
    if policy == WorkflowPolicy::ConsumerV1
        && input
            .published_generator_lock
            .is_some_and(|lock| lock.source_preview.is_some())
    {
        return Err(OrchestratorError::Contract {
            problem: "consumer_preview_controller_unsupported".to_owned(),
        });
    }
    let report_helper_version = input
        .published_generator_lock
        .map_or(input.consumer_release_version, |lock| {
            if policy == WorkflowPolicy::VelnorRepositoryV1 {
                lock.source_preview
                    .as_ref()
                    .map_or(lock.generator.version.as_str(), |preview| {
                        preview.receipt.version.as_str()
                    })
            } else {
                lock.generator.version.as_str()
            }
        })
        .to_owned();
    let acquire = match policy {
        WorkflowPolicy::ConsumerV1 => Some(consumer_acquire_step(
            input.label,
            &report_helper_version,
            input.discovery,
        )?),
        WorkflowPolicy::VelnorRepositoryV1 => {
            let lock =
                input
                    .published_generator_lock
                    .ok_or_else(|| OrchestratorError::Contract {
                        problem: "required_verdict_requires_published_generator_lock".to_owned(),
                    })?;
            if lock.source_preview.is_some() {
                Some(velnor_actions_workflow_renderer::steps::protected_preview_acquire_step()?)
            } else {
                Some(crate::attach::published_lock_acquire_step(
                    lock,
                    input.label,
                    &lock.generator.version,
                )?)
            }
        }
    };
    Ok((report_helper_version, acquire))
}

fn prepare_workflow(input: &WorkflowBuildInput<'_>) -> Result<PreparedWorkflow, OrchestratorError> {
    wire_w1::vet_step_syntax(StepSyntax::JobMatrix)?;
    wire_w1::vet_step_syntax(StepSyntax::NativeParallelism)?;
    wire_w1::vet_step_syntax(StepSyntax::CacheMode)?;
    crate::rust_suites_emit::validate_suites_against_repo(
        input.root,
        input.config,
        input.discovery,
    )?;
    let catalog = input.catalog;
    let policy = input.config.workflow.policy;
    let (report_helper_version, acquire) = controller_acquisition(input, policy)?;
    let generator_version = env!("CARGO_PKG_VERSION");
    let verify = crate::verify::verify_kinds(&input.config.workflow.verify.jobs)?;
    let support = support_workflow(policy, input.discovery, &verify);
    let private =
        crate::config::private_wiring::PrivateWiring::load(input.root, input.config, catalog)?;
    let workflow_tasks = lane_workflow_tasks(input, acquire.as_ref(), private.as_ref())?;
    let format = wire_w1::workspace_format_step(input.discovery, catalog)?;
    let rust = match (plan_uses_rust(input.discovery, policy), format.is_some()) {
        (false, false) => PlanRustNeed::None,
        (_, true) => PlanRustNeed::CompilerAndComponents,
        (true, false) => PlanRustNeed::Compiler,
    };
    let needs = PlanJobToolNeeds {
        rust,
        all_features: plan_uses_all_features(input.discovery),
        nextest: plan_uses_nextest(input.discovery),
        opentofu: plan_uses_opentofu(input.discovery),
        gh: policy == WorkflowPolicy::VelnorRepositoryV1
            || !input.discovery.native_checks.is_empty(),
        resolve_qualification_predecessor: policy == WorkflowPolicy::VelnorRepositoryV1,
    };
    Ok(PreparedWorkflow {
        private,
        catalog: catalog.clone(),
        helper_catalog: helper_catalog(input, catalog)?,
        report_helper_version,
        generator_version,
        policy,
        workflow_tasks,
        verify,
        support,
        acquire,
        format,
        needs,
        state_recipes: BTreeMap::new(),
    })
}

fn build_workflow_plan(
    input: &WorkflowBuildInput<'_>,
    prepared: &mut PreparedWorkflow,
) -> Result<Job, OrchestratorError> {
    let tofu_roots = crate::tofu_cache::tofu_cache_roots(input.discovery)?;
    let mut plan = plan_job(
        input.label,
        prepared.acquire.clone(),
        &prepared.helper_catalog,
        prepared.needs,
        input.fetch_roots,
        &tofu_roots,
        prepared.policy,
    )?;
    if let Some(private) = prepared.private.as_ref() {
        private.insert_into_job(&mut plan, input.fetch_roots)?;
    }
    let controller = input
        .published_generator_lock
        .filter(|_| prepared.policy == WorkflowPolicy::VelnorRepositoryV1)
        .and_then(|lock| lock.source_preview.as_ref())
        .map_or_else(
            || velnor_actions_workflow_renderer::render::ControllerLocator::PinnedRelease {
                version: prepared.report_helper_version.clone(),
                path: format!(
                    "{}{version}",
                    velnor_actions_workflow_renderer::steps::STAGED_BINARY_PREFIX,
                    version = prepared.report_helper_version
                ),
            },
            |preview| {
                velnor_actions_workflow_renderer::render::ControllerLocator::ProtectedBaseDispatch {
                    preview: Box::new(preview.clone()),
                    path: velnor_actions_workflow_renderer::render::PROTECTED_BASE_CONTROLLER_PATH
                        .to_owned(),
                }
            },
        );
    super::insert_format_report_steps(
        &mut plan,
        wire_w1::workspace_format_report_steps(input.discovery, &controller)?,
    );
    plan.timeout_minutes =
        velnor_actions_contract::JobTimeout::new(input.config.workflow.plan_timeout_minutes)
            .map_err(|err| OrchestratorError::Contract {
                problem: err.to_string(),
            })?;
    if let Some(format) = prepared.format.take() {
        insert_format_step(
            &mut plan,
            format,
            prepared.policy == WorkflowPolicy::VelnorRepositoryV1,
        );
    }
    if prepared.policy == WorkflowPolicy::VelnorRepositoryV1
        || !input.discovery.native_checks.is_empty()
    {
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
        prepared.generator_version,
        input
            .published_generator_lock
            .and_then(|lock| lock.source_preview.as_ref()),
        prepared.private.as_ref(),
    )?;
    let mut required_ids: Vec<String> = built.jobs.iter().map(|(id, _)| id.clone()).collect();
    for (id, job) in built.jobs {
        jobs.insert(id, job);
    }
    prepared.state_recipes = built.state_recipes;
    for (id, job) in built.producers.into_iter().chain(built.ledger_cli) {
        jobs.insert(id, job);
    }
    for (id, job) in check_jobs::build_check_jobs(
        prepared.policy,
        input.discovery,
        &prepared.catalog,
        input.published_generator_lock,
        &prepared.report_helper_version,
    )? {
        required_ids.push(id.clone());
        jobs.insert(id, job);
    }
    for (id, job) in crate::config::gradle_jobs::build_jobs(
        input.config,
        &input.discovery.gradle_roots,
        prepared.acquire.as_ref(),
        prepared.private.as_ref(),
    )? {
        required_ids.push(id.clone());
        jobs.insert(id, job);
    }
    let suite_build = crate::rust_suites_emit::build_ci_jobs(
        input.root,
        input.config,
        input.label,
        input.discovery,
        &prepared.catalog,
    )?;
    for (id, job) in suite_build.jobs {
        jobs.insert(id, job);
    }
    required_ids.extend(suite_build.required_ids);
    attach_state_roots(input, prepared, &mut jobs)?;
    crate::workflow_task_jobs::insert_jobs(&mut jobs, &prepared.workflow_tasks)?;
    let (required_catalog, required_acquire) = required_controller(input, prepared)?;
    insert_gate_jobs(
        &mut jobs,
        input.label,
        &required_ids,
        required_acquire,
        &prepared.catalog,
        &required_catalog,
        input.config.required_checkout_history(),
    )?;
    let mut drivers = built.drivers;
    drivers.extend(suite_build.drivers);
    wire_w1::check_crate_mbx_gating(&jobs, &drivers, &prepared.catalog)?;
    Ok(jobs)
}

#[path = "workflow_build_authority.rs"]
mod authority;
use authority::{attach_state_roots, helper_catalog, required_controller};

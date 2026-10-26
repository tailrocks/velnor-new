// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_actionlint::StepSyntax;
use velnor_actions_contract::GeneratorLock;
use velnor_actions_contract::{
    ExecutionMode, Job, Step, ValidatorKind, VelnorConfig, VelnorSupportWorkflow, WorkflowIr,
    WorkflowPolicy, expand_workflow,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::{FINAL_JOB_ID, PLAN_JOB_ID};
use velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::pins::consumer_acquire_step;
use crate::workflow_jobs::{PlanJobToolNeeds, PlanRustNeed, plan_job};

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
    pub(crate) catalog: &'a ToolCatalog,
}

struct PreparedWorkflow {
    private: Option<crate::config::private_wiring::PrivateWiring>,
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
    let mut jobs = build_workflow_jobs(input, &mut prepared, plan)?;
    if input.config.workflow.checkout_lfs {
        wire_w1::apply_checkout_lfs(&mut jobs);
    }
    let mut ir = workflow_ir(input.config, input.branch, jobs, prepared.policy);
    let canonical_mise_setup = crate::pins::resolve_mise_setup(input.config, input.label)?;
    let expanded_authority_jobs = expanded_authority_variants(&ir, input.config, prepared.policy)?;
    let runtime = workflow_context::canonical_runtime(
        &prepared.catalog,
        &prepared.report_helper_version,
        prepared.needs.rust.has_compiler(),
    )?;
    let authority_steps = velnor_actions_workflow_renderer::render::CanonicalAuthoritySteps::
        from_native_jobs_with_mise_setup(
            &ir.jobs,
            prepared.policy,
            &canonical_mise_setup,
            &expanded_authority_jobs,
            runtime.clone(),
        )?;
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
            &context.staged_binary,
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

fn expanded_authority_variants(
    ir: &WorkflowIr,
    config: &VelnorConfig,
    policy: WorkflowPolicy,
) -> Result<Vec<(Job, Option<Job>)>, OrchestratorError> {
    let dispatches = [
        None,
        Some(ExecutionMode::Hosted),
        Some(ExecutionMode::ScaleSet),
        Some(ExecutionMode::Both),
    ];
    let mut authorities = Vec::new();
    for dispatch in dispatches {
        let expanded = match expand_workflow(ir, config, dispatch) {
            Ok(expanded) => expanded,
            Err(error) if dispatch.is_none() => {
                return Err(OrchestratorError::Contract {
                    problem: error.to_string(),
                });
            }
            Err(_) => continue,
        };
        let required = expanded.jobs.get(FINAL_JOB_ID).cloned().ok_or_else(|| {
            OrchestratorError::Contract {
                problem: "workflow_missing_required_authority_snapshot".to_owned(),
            }
        })?;
        let producer_plan = if policy == WorkflowPolicy::VelnorRepositoryV1 {
            Some(expanded.jobs.get(PLAN_JOB_ID).cloned().ok_or_else(|| {
                OrchestratorError::Contract {
                    problem: "workflow_missing_plan_authority_snapshot".to_owned(),
                }
            })?)
        } else {
            None
        };
        authorities.push((required, producer_plan));
    }
    Ok(authorities)
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
    let report_helper_version = input
        .published_generator_lock
        .map_or(input.consumer_release_version, |lock| {
            lock.generator.version.as_str()
        })
        .to_owned();
    let generator_version = env!("CARGO_PKG_VERSION");
    let policy = input.config.workflow.policy;
    let verify = crate::verify::verify_kinds(&input.config.workflow.verify.jobs)?;
    let support = support_workflow(policy, input.discovery, &verify);
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
            Some(crate::attach::published_lock_acquire_step(
                lock,
                input.label,
                &lock.generator.version,
            )?)
        }
    };
    let private =
        crate::config::private_wiring::PrivateWiring::load(input.root, input.config, catalog)?;
    let workflow_tasks = crate::workflow_task_jobs::policies(
        input.root,
        input.config,
        input.discovery,
        catalog,
        input.fetch_roots,
        acquire.as_ref(),
        private.as_ref(),
    )?;
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
    let tofu_roots = crate::tofu_cache::tofu_cache_roots(input.discovery)?;
    let mut plan = plan_job(
        input.label,
        prepared.acquire.clone(),
        &prepared.catalog,
        prepared.needs,
        input.fetch_roots,
        &tofu_roots,
        prepared.policy == WorkflowPolicy::VelnorRepositoryV1,
    )?;
    if let Some(private) = prepared.private.as_ref() {
        private.insert_into_job(&mut plan, input.fetch_roots)?;
    }
    super::insert_format_report_steps(
        &mut plan,
        wire_w1::workspace_format_report_steps(input.discovery, &prepared.report_helper_version)?,
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
        prepared.private.as_ref(),
    )?;
    let mut required_ids: Vec<String> = built.jobs.iter().map(|(id, _)| id.clone()).collect();
    for (id, job) in built.jobs {
        jobs.insert(id, job);
    }
    for (id, job) in built.seeds {
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

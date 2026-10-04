//! Ordered source and tool producers around the computation job graph.

use std::{collections::BTreeMap, path::Path};

use velnor_actions_contract::{CompiledSourceHelper, Job, Step, VelnorConfig, WorkflowPolicy};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::{PLAN_JOB_ID, RenderContext};

use crate::{OrchestratorError, discover::Discovery};

pub(super) struct BuildScope<'a> {
    pub root: &'a Path,
    pub config: &'a VelnorConfig,
    pub branch: &'a str,
    pub label: &'a str,
    pub discovery: &'a Discovery,
    pub fetch_roots: &'a [String],
    pub catalog: &'a ToolCatalog,
    pub version: &'a str,
}

pub(super) struct BuiltJobs {
    pub jobs: BTreeMap<String, Job>,
    pub helpers: Vec<CompiledSourceHelper>,
    pub context: RenderContext,
    pub receipt_drafts: Vec<crate::cache_producer_workflow::DraftCacheProducerWorkflow>,
    pub mbx_finalization: crate::workflow_mbx_finalize::MbxFinalization,
}

/// Insert source producers before gates, support jobs, and tool cohorts.
pub(super) fn build_jobs(scope: &BuildScope<'_>) -> Result<BuiltJobs, OrchestratorError> {
    let policy = scope.config.workflow.policy;
    let acquire = match policy {
        WorkflowPolicy::ConsumerV1 => Some(crate::pins::consumer_acquire_step(
            scope.label,
            scope.version,
            scope.discovery,
        )?),
        WorkflowPolicy::VelnorRepositoryV1 => None,
    };
    let use_rust = super::plan_uses_rust(scope.discovery, policy);
    let (mut jobs, mut helpers) = computation_jobs(scope, acquire.as_ref(), use_rust)?;
    let crate_ids = jobs
        .keys()
        .filter(|id| id.as_str() != PLAN_JOB_ID)
        .cloned()
        .collect::<Vec<_>>();
    let setup = crate::pins::resolve_mise_setup(scope.config, scope.label)?;
    helpers.extend(crate::tofu_producer_job::insert_producers(
        &mut jobs,
        scope.discovery,
        scope.catalog,
        scope.label,
        scope.config,
        scope.version,
    )?);
    let native = crate::native_source_graph::insert_producers(
        &mut jobs,
        scope.discovery,
        scope.catalog,
        scope.config,
        scope.version,
    )?;
    helpers.extend(native.source_helpers);
    helpers.extend(crate::source_prep::producer::insert_producers(
        &mut jobs,
        scope.root,
        scope.discovery,
        scope.catalog,
        &setup,
        scope.fetch_roots,
        scope.version,
    )?);
    super::insert_gate_jobs(
        &mut jobs,
        scope.label,
        scope.branch,
        &crate_ids,
        acquire,
        scope.catalog,
    )?;
    let context = materialize_support(scope, &mut jobs, use_rust)?;
    let mut authority = helpers.clone();
    authority.extend(crate::tofu_cached_init::collect_job_records(
        jobs.values(),
        scope.version,
    )?);
    let tool_helpers = crate::workflow_tool_producer::insert_producers(
        &mut jobs,
        scope.discovery,
        scope.catalog,
        &setup,
        scope.version,
        &authority,
    )?;
    helpers.extend(tool_helpers);
    finalize_jobs(scope, jobs, helpers, context, native.receipt_drafts, &setup)
}

fn computation_jobs(
    scope: &BuildScope<'_>,
    acquire: Option<&Step>,
    use_rust: bool,
) -> Result<(BTreeMap<String, Job>, Vec<CompiledSourceHelper>), OrchestratorError> {
    let plan = super::build_plan_job(
        scope.label,
        acquire.cloned(),
        scope.catalog,
        use_rust,
        super::plan_uses_mbx(scope.discovery),
        super::plan_uses_nextest(scope.discovery),
        super::plan_uses_opentofu(scope.discovery),
        scope.fetch_roots,
        scope.discovery,
    )?;
    let custom_tasks: &[String] = scope
        .config
        .stacks
        .rust
        .as_ref()
        .map_or(&[], |rust| &rust.custom_tasks);
    let built = crate::crate_jobs::build_crate_jobs(
        scope.label,
        scope.config.workflow.policy,
        scope.discovery,
        scope.catalog,
        scope.fetch_roots,
        custom_tasks,
        acquire,
        scope.config.workflow.max_parallel_jobs,
    )?;
    let mut jobs = BTreeMap::from([(PLAN_JOB_ID.to_owned(), plan)]);
    jobs.extend(built.jobs);
    super::wire_w1::check_crate_mbx_gating(&jobs, &built.drivers, &built.helper_records)?;
    Ok((jobs, built.helper_records))
}

fn materialize_support(
    scope: &BuildScope<'_>,
    jobs: &mut BTreeMap<String, Job>,
    use_rust: bool,
) -> Result<RenderContext, OrchestratorError> {
    let policy = scope.config.workflow.policy;
    let support = match policy {
        WorkflowPolicy::ConsumerV1 => None,
        WorkflowPolicy::VelnorRepositoryV1 => {
            Some(policy.support_workflow(scope.config.workflow.generator_validation))
        }
    };
    let context = super::workflow_context::render_context(
        scope.config,
        scope.label,
        scope.version,
        scope.catalog,
        use_rust,
    )?;
    velnor_actions_workflow_renderer::render::merge_support_jobs(jobs, support.as_ref(), &context)?;
    Ok(context)
}

fn finalize_jobs(
    scope: &BuildScope<'_>,
    jobs: BTreeMap<String, Job>,
    mut helpers: Vec<CompiledSourceHelper>,
    context: RenderContext,
    mut receipt_drafts: Vec<crate::cache_producer_workflow::DraftCacheProducerWorkflow>,
    setup: &velnor_actions_workflow_renderer::MiseSetup,
) -> Result<BuiltJobs, OrchestratorError> {
    let mut authority = Vec::new();
    let mut fresh =
        super::workflow_context::collect_rust_job_helpers(jobs.values(), scope.version)?;
    fresh.extend(crate::tofu_cached_init::collect_job_records(
        jobs.values(),
        scope.version,
    )?);
    for record in helpers.iter().cloned().chain(fresh) {
        if !authority.contains(&record) {
            authority.push(record);
        }
    }
    let mbx_finalization = crate::workflow_mbx_finalize::finalize(
        &jobs,
        scope.discovery,
        scope.catalog,
        setup,
        scope.version,
        &authority,
        scope.fetch_roots,
    )?;
    helpers.extend(mbx_finalization.source_helpers.iter().cloned());
    receipt_drafts.extend(mbx_finalization.receipt_drafts.iter().cloned());
    Ok(BuiltJobs {
        jobs,
        helpers,
        context,
        receipt_drafts,
        mbx_finalization,
    })
}

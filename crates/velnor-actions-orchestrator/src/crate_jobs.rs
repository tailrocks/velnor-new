//! Crate-job construction: one job per crate with parallel check steps.
//!
//! Every Rust package/configuration owns one job: checkout, tools, locked sources
//! and caches are prepared once, then format, lint, tests, docs and doctests run
//! as ordered or parallel obligations. `OpenTofu` keeps one bounded job per root.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CrateJob, Job, JobTimeout, ProposedTask, RustCrateConfig, Stack, Step, VelnorConfig,
    WorkflowPolicy, crate_display_name, tofu_display_name,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::steps::CompileDriver as RenderDriver;
#[cfg(test)]
use {velnor_actions_mise::PinnedTool, velnor_actions_workflow_renderer::render::PLAN_JOB_ID};

use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;
use crate::crate_job_ids::{
    JobOwners, group_is_tofu, group_runnable, qualification_job_id_for_member,
};
use crate::discover::Discovery;
use crate::internal::internal;

#[path = "crate_jobs_inputs.rs"]
mod inputs;
pub(crate) use inputs::CrateJobInputs;
#[cfg(test)]
pub(crate) use inputs::build_crate_jobs_with;

#[path = "dependency_seed.rs"]
pub(crate) mod dependency_seed;

#[path = "crate_jobs_build_jobs.rs"]
mod build_jobs;
use build_jobs::{cap_cargo_jobs, disambiguate_rust_job_names, reject_unknown_crates};

#[path = "crate_jobs_steps.rs"]
mod job_steps;

static NO_CRATE_CONFIGS: BTreeMap<String, RustCrateConfig> = BTreeMap::new();

#[path = "crate_jobs_obligations.rs"]
mod obligations;
use obligations::obligations_for;
#[cfg(test)]
use obligations::{gates_for, obligation_rank};

#[path = "crate_jobs_render.rs"]
mod render;
use render::render_job;

#[path = "crate_jobs_stage.rs"]
mod stage;

#[path = "crate_jobs_dependencies.rs"]
mod crate_jobs_dependencies;

#[path = "crate_jobs_qualification_cache.rs"]
mod qualification_cache;

#[cfg(test)]
pub(crate) use stage::needs;
pub(crate) use stage::{is_mbx, is_nextest, is_opentofu, is_rust};

/// Built crate jobs plus their driver selections for MBX gating.
#[derive(Default)]
pub(crate) struct CrateBuild {
    /// These dependency seed jobs implement cache decision 2 and are not part of `Required`.
    pub(crate) seeds: Vec<(String, Job)>,
    /// `(job_id, job)` pairs in deterministic job-ID order.
    pub(crate) jobs: Vec<(String, Job)>,
    /// Render-driver selection per crate job ID.
    pub(crate) drivers: BTreeMap<String, RenderDriver>,
}

/// Build crate jobs with the repository's workflow and Rust-stack config.
/// # Errors
pub(crate) fn build_for_workflow(
    input: CrateJobInputs<'_>,
    qualification: bool,
    private: Option<&PrivateWiring>,
    seed_timeout: JobTimeout,
) -> Result<CrateBuild, OrchestratorError> {
    build_crate_jobs_inner(input, qualification, private, seed_timeout)
}

/// Make one `ToFu` Qualification executor for each matrix entry that has tasks.
pub(crate) fn build_qualification_for_workflow(
    config: &VelnorConfig,
    label: &str,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    acquire: Option<&Step>,
    helper_version: &str,
) -> Result<CrateBuild, OrchestratorError> {
    let inputs = workflow_job_inputs(
        config,
        label,
        discovery,
        catalog,
        fetch_roots,
        acquire,
        helper_version,
    )?;
    build_qualification_crate_jobs(inputs)
}

/// Shared rendering inputs for grouped and `ToFu` Qualification jobs.
pub(crate) fn workflow_job_inputs<'a>(
    config: &'a VelnorConfig,
    label: &'a str,
    discovery: &'a Discovery,
    catalog: &'a ToolCatalog,
    fetch_roots: &'a [String],
    acquire: Option<&'a Step>,
    helper_version: &'a str,
) -> Result<CrateJobInputs<'a>, OrchestratorError> {
    let crates = config
        .stacks
        .rust
        .as_ref()
        .map_or(&NO_CRATE_CONFIGS, |rust| &rust.crates);
    reject_unknown_crates(crates, discovery)?;
    Ok(CrateJobInputs {
        label,
        policy: config.workflow.policy,
        discovery,
        catalog,
        fetch_roots,
        acquire,
        max_parallel_jobs: config.workflow.max_parallel_jobs,
        timeout: JobTimeout::new(config.workflow.crate_timeout_minutes)
            .map_err(crate::internal::internal_contract)?,
        helper_version,
        seed_parallel_targets: config
            .stacks
            .rust
            .as_ref()
            .is_some_and(|rust| rust.seed_parallel_targets),
        crates,
    })
}

/// Build one ordered IR job per runnable Rust package/configuration or `ToFu` root.
///
/// One Rust job owns every runnable package task for its configuration.
/// The plan job owns package-less workspace Format tasks.
/// Each job runs Build before its parallel checks.
/// The report step follows the parallel join.
/// The acquire step stages the helper for each report wrapper.
/// Callers without that step provision the helper separately.
///
/// # Errors
///
/// Returns contract, render-context, or tool-request errors.
#[cfg(test)]
pub(crate) fn build_crate_jobs(input: CrateJobInputs<'_>) -> Result<CrateBuild, OrchestratorError> {
    build_crate_jobs_with(input, None)
}

#[cfg(test)]
fn build_crate_jobs_with_seed_timeout(
    input: CrateJobInputs<'_>,
    private: Option<&PrivateWiring>,
    seed_timeout: JobTimeout,
) -> Result<CrateBuild, OrchestratorError> {
    build_crate_jobs_inner(input, false, private, seed_timeout)
}

/// Make one `ToFu` Qualification job for each matrix entry that has tasks.
///
/// Rust Qualification obligations use the same package job as normal CI.
pub(crate) fn build_qualification_crate_jobs(
    input: CrateJobInputs<'_>,
) -> Result<CrateBuild, OrchestratorError> {
    let mut tasks: Vec<&ProposedTask> = input
        .discovery
        .proposals
        .iter()
        .filter(|task| is_runnable(task) && Stack::from_id(&task.stack_id) == Some(Stack::Tofu))
        .collect();
    tasks.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    reject_unknown_crates(input.crates, input.discovery)?;
    let grouped = group_runnable(&input.discovery.proposals)?;
    let owners = JobOwners::from_grouped(&input.discovery.proposals, &grouped);
    let mut jobs = Vec::with_capacity(tasks.len());
    let mut drivers = BTreeMap::new();
    let mut tofu_ids = Vec::new();
    for task in tasks {
        let job_id =
            qualification_job_id_for_member(task).map_err(crate::internal::internal_contract)?;
        let needs = crate_jobs_dependencies::external_needs(
            std::slice::from_ref(&task),
            &owners,
            &job_id,
            velnor_actions_contract::WorkflowEvent::Qualification,
        )?;
        let (job, driver, use_opentofu) = build_job_for_members(
            input,
            &job_id,
            std::slice::from_ref(&task),
            needs,
            true,
            None,
        )?;
        drivers.insert(job_id.clone(), driver);
        if use_opentofu {
            tofu_ids.push(job_id.clone());
        }
        jobs.push((job_id, job));
    }
    stage::stage_tofu_root_jobs(&mut jobs, &tofu_ids, input.max_parallel_jobs);
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    if jobs.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(internal("qualification_crate_job_id_collision"));
    }
    Ok(CrateBuild {
        seeds: Vec::new(),
        jobs,
        drivers,
    })
}

fn build_crate_jobs_inner(
    input: CrateJobInputs<'_>,
    qualification: bool,
    private: Option<&PrivateWiring>,
    seed_timeout: JobTimeout,
) -> Result<CrateBuild, OrchestratorError> {
    let grouped = group_runnable(&input.discovery.proposals)?;
    let owners = JobOwners::from_grouped(&input.discovery.proposals, &grouped);
    let mut jobs = Vec::with_capacity(grouped.len());
    let mut drivers = BTreeMap::new();
    let mut tofu_ids = Vec::new();
    let mut manifests = BTreeMap::new();
    let workspaces = dependency_seed::workspaces_for_discovery(input.discovery, input.fetch_roots);
    for (key, tasks) in &grouped {
        let job_id = owners
            .group_id(key)
            .ok_or_else(|| internal("crate_job_id_missing"))?
            .clone();
        let is_tofu = group_is_tofu(tasks);
        if !is_tofu {
            manifests.insert(job_id.clone(), tasks[0].identity.unit_path.clone());
        }
        let needs = crate_jobs_dependencies::external_needs(
            tasks,
            &owners,
            &job_id,
            velnor_actions_contract::WorkflowEvent::PullRequest,
        )?;
        let (job, driver, use_opentofu) = build_job_for_members(
            input,
            &job_id,
            tasks,
            needs,
            qualification && !is_tofu,
            private,
        )?;
        drivers.insert(job_id.clone(), driver);
        if use_opentofu {
            tofu_ids.push(job_id.clone());
        }
        jobs.push((job_id, job));
    }
    stage::stage_tofu_root_jobs(&mut jobs, &tofu_ids, input.max_parallel_jobs);
    disambiguate_rust_job_names(&mut jobs, &manifests)?;
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    let seeds = dependency_seed::seed_jobs(&input, &workspaces, private, seed_timeout)?;
    for (id, _) in &seeds {
        drivers.insert(id.clone(), RenderDriver::Mbx);
    }
    Ok(CrateBuild {
        seeds,
        jobs,
        drivers,
    })
}

/// Build and validate one crate job from its exact obligation slice.
fn build_job_for_members(
    input: CrateJobInputs<'_>,
    job_id: &str,
    tasks: &[&ProposedTask],
    needs: Vec<String>,
    qualification: bool,
    private: Option<&PrivateWiring>,
) -> Result<(Job, RenderDriver, bool), OrchestratorError> {
    let first = tasks.first().ok_or_else(|| internal("crate_empty"))?;
    let manifest = first.identity.unit_path.clone();
    let tofu = group_is_tofu(tasks);
    let display = if tofu {
        tofu_display_name(&first.display_name)
    } else {
        crate_display_name(&first.display_name, &manifest, &first.configuration)
    };
    let use_rust = tasks.iter().any(|task| is_rust(task));
    let use_mbx = tasks.iter().any(|task| is_mbx(task));
    let use_nextest = tasks.iter().any(|task| is_nextest(task));
    let use_opentofu = tasks.iter().any(|task| is_opentofu(task));
    let driver = if use_mbx {
        RenderDriver::Mbx
    } else {
        RenderDriver::Cargo
    };
    let model = CrateJob {
        job_id: job_id.to_owned(),
        display_name: display,
        package_name: first.display_name.clone(),
        package_id: first.identity.unit_id.clone(),
        manifest,
        configuration: first.configuration.clone(),
        obligations: obligations_for(tasks, input.catalog)?,
    };
    model.validate()?;
    let job = render_job(
        input.label,
        input.policy,
        &model,
        input.catalog,
        input.fetch_roots,
        &crate::tofu_cache::tofu_cache_roots(input.discovery)?,
        use_rust,
        use_mbx,
        use_nextest,
        use_opentofu,
        input.acquire,
        input.max_parallel_jobs,
        input.helper_version,
        input.timeout,
        needs,
        qualification,
    )?;
    let mut job = job;
    if use_rust && let Some(private) = private {
        let suite = crate::matrix_step::suite_for_package(&model.package_name);
        let needs_validators =
            crate::matrix_step::crate_needs_generate_validators(input.policy, suite);
        let roots = crate::source_prep::crate_fetch_roots(
            &model.manifest,
            input.fetch_roots,
            input.policy == WorkflowPolicy::VelnorRepositoryV1 && needs_validators,
        );
        private.insert_into_job(&mut job, &roots)?;
    }
    if let Some(seed) =
        job_steps::parallel_target_seed_step(&input, &model, use_rust, use_opentofu)?
        && let Some(index) = job.steps.iter().position(|step| step.parallel)
    {
        job.steps.insert(index, seed);
    }
    if use_rust && let Some(options) = input.crates.get(&first.display_name) {
        for step in &mut job.steps {
            cap_cargo_jobs(step, options.build_jobs);
        }
    }
    Ok((job, driver, use_opentofu))
}

/// True when the task becomes a crate-job obligation.
///
/// Test-less tasks carry no command and package-less workspace tasks
/// belong to the plan job, so neither is emitted as an obligation.
/// Shared with `plan` so its obligation list matches emission exactly.
pub(crate) fn is_runnable(task: &ProposedTask) -> bool {
    !task.no_targets
        && !task.identity.unit_id.is_empty()
        && !matches!(
            Stack::from_id(&task.stack_id),
            Some(Stack::Mise | Stack::Native)
        )
}

#[cfg(test)]
#[path = "crate_jobs_test_modules.rs"]
mod test_modules;
#[cfg(test)]
pub(crate) use test_modules::crate_jobs_tests;

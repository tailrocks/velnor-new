//! Rust packages use one job for all task configurations.
//! `ToFu` packages use one job for each configuration.
//! Each job runs its tasks in the specified order.
//! The job uses the tools, caches, and setup that its tasks need.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CrateJob, CrateObligation, Job, JobTimeout, ProposedTask, Stack, Step, VelnorConfig,
    WorkflowPolicy, crate_display_name, matrix_id_for_task_group, matrix_key_for_id,
    tofu_display_name,
};
#[cfg(test)]
use velnor_actions_mise::PinnedTool;
use velnor_actions_mise::{TaskCacheMode, ToolCatalog};
#[cfg(test)]
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::steps::CompileDriver as RenderDriver;

use crate::OrchestratorError;
use crate::crate_job_ids::{
    JobOwners, group_is_tofu, group_runnable, qualification_job_id_for_member,
};
use crate::discover::Discovery;
use crate::internal::internal;
use crate::matrix_step::step_name_for;
use crate::obligation_order::obligation_order_key;

#[path = "crate_jobs_render.rs"]
mod render;
use render::render_job;

#[path = "crate_jobs_stage.rs"]
mod stage;

#[path = "crate_jobs_dependencies.rs"]
mod crate_jobs_dependencies;

#[cfg(test)]
pub(crate) use stage::needs;
pub(crate) use stage::{is_mbx, is_nextest, is_opentofu, is_rust};

/// Built crate jobs plus their driver selections for MBX gating.
#[derive(Default)]
pub(crate) struct CrateBuild {
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
) -> Result<CrateBuild, OrchestratorError> {
    build_crate_jobs_inner(input, qualification)
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
    build_qualification_crate_jobs(workflow_job_inputs(
        config,
        label,
        discovery,
        catalog,
        fetch_roots,
        acquire,
        helper_version,
    )?)
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
        cache_policy: config.workflow.pull_request_cache_policy,
    })
}

/// All authority and rendering inputs for one crate-job build.
#[derive(Clone, Copy)]
pub(crate) struct CrateJobInputs<'a> {
    pub(crate) label: &'a str,
    pub(crate) policy: WorkflowPolicy,
    pub(crate) discovery: &'a Discovery,
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) fetch_roots: &'a [String],
    pub(crate) acquire: Option<&'a Step>,
    pub(crate) max_parallel_jobs: u32,
    /// Literal timeout for each emitted crate job.
    pub(crate) timeout: JobTimeout,
    pub(crate) helper_version: &'a str,
    pub(crate) cache_policy: velnor_actions_contract::PullRequestCachePolicy,
}

/// Build one ordered IR job per runnable crate from discovery proposals.
///
/// Tasks without test targets carry no command and stay out; the
/// package-less workspace Format scope belongs to the plan job, so its
/// tasks stay out too. Obligations order Format, Clippy, build, tests,
/// doctests, docs (shards by task ID); gates keep same-crate edges
/// only, so every gate references a strictly earlier obligation. The
/// acquire step stages the helper every report wrapper invokes;
/// callers without one (pre-seed, lock attach) provision separately.
///
/// # Errors
///
/// Returns contract, render-context, or tool-request errors.
#[cfg(test)]
pub(crate) fn build_crate_jobs(input: CrateJobInputs<'_>) -> Result<CrateBuild, OrchestratorError> {
    build_crate_jobs_inner(input, false)
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
        let (job, driver, use_opentofu) =
            build_job_for_members(input, &job_id, std::slice::from_ref(&task), needs, true)?;
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
    Ok(CrateBuild { jobs, drivers })
}

/// Add manifest context only when two Rust packages share one display name.
fn disambiguate_rust_job_names(
    jobs: &mut [(String, Job)],
    manifests: &BTreeMap<String, String>,
) -> Result<(), OrchestratorError> {
    let mut counts = BTreeMap::<String, usize>::new();
    for (job_id, job) in jobs.iter() {
        if job_id.starts_with("rust-") {
            *counts.entry(job.display_name.clone()).or_default() += 1;
        }
    }
    for (job_id, job) in jobs.iter_mut() {
        if job_id.starts_with("rust-")
            && counts
                .get(&job.display_name)
                .is_some_and(|count| *count > 1)
        {
            let manifest = manifests
                .get(job_id)
                .ok_or_else(|| internal("rust_job_manifest_missing"))?;
            job.display_name = format!("{} ({manifest})", job.display_name);
        }
    }
    Ok(())
}

fn build_crate_jobs_inner(
    input: CrateJobInputs<'_>,
    qualification: bool,
) -> Result<CrateBuild, OrchestratorError> {
    let grouped = group_runnable(&input.discovery.proposals)?;
    let owners = JobOwners::from_grouped(&input.discovery.proposals, &grouped);
    let mut jobs = Vec::with_capacity(grouped.len());
    let mut drivers = BTreeMap::new();
    let mut tofu_ids = Vec::new();
    let mut manifests = BTreeMap::new();
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
        let (job, driver, use_opentofu) =
            build_job_for_members(input, &job_id, tasks, needs, qualification && !is_tofu)?;
        drivers.insert(job_id.clone(), driver);
        if use_opentofu {
            tofu_ids.push(job_id.clone());
        }
        jobs.push((job_id, job));
    }
    stage::stage_tofu_root_jobs(&mut jobs, &tofu_ids, input.max_parallel_jobs);
    disambiguate_rust_job_names(&mut jobs, &manifests)?;
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(CrateBuild { jobs, drivers })
}

/// Build and validate one crate job from its exact obligation slice.
fn build_job_for_members(
    input: CrateJobInputs<'_>,
    job_id: &str,
    tasks: &[&ProposedTask],
    needs: Vec<String>,
    qualification: bool,
) -> Result<(Job, RenderDriver, bool), OrchestratorError> {
    let first = tasks.first().ok_or_else(|| internal("crate_empty"))?;
    let manifest = first.identity.unit_path.clone();
    let tofu = group_is_tofu(tasks);
    let display = if tofu {
        tofu_display_name(&first.display_name)
    } else {
        crate_display_name(&first.display_name, &manifest)
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
        obligations: obligations_for(tasks, input.catalog)?,
    };
    model.validate()?;
    let job = render_job(
        input.label,
        input.policy,
        &model,
        input.catalog,
        input.fetch_roots,
        use_rust,
        use_mbx,
        use_nextest,
        use_opentofu,
        input.acquire,
        input.max_parallel_jobs,
        input.helper_version,
        input.cache_policy,
        input.timeout,
        needs,
        qualification,
    )?;
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
        && Stack::from_id(&task.stack_id) != Some(Stack::Mise)
}

/// Rank one proposal through the shared task-order key.
#[cfg(test)]
fn obligation_rank(task: &ProposedTask) -> u32 {
    obligation_order_key(&task.stack_id, &task.task_kind, &task.task_id).0
}

/// Ordered validated obligations for one crate's tasks.
fn obligations_for(
    tasks: &[&ProposedTask],
    catalog: &ToolCatalog,
) -> Result<Vec<CrateObligation>, OrchestratorError> {
    let executed: BTreeSet<&str> = tasks.iter().map(|task| task.task_id.as_str()).collect();
    let mut ordered = tasks.to_vec();
    ordered.sort_by(|left, right| {
        obligation_order_key(&left.stack_id, &left.task_kind, &left.task_id).cmp(
            &obligation_order_key(&right.stack_id, &right.task_kind, &right.task_id),
        )
    });
    let mut obligations = Vec::with_capacity(ordered.len());
    for task in ordered {
        obligations.push(obligation_for(task, &executed, catalog)?);
    }
    Ok(obligations)
}

/// One obligation: identities, same-crate gates, fixed argv.
fn obligation_for(
    task: &ProposedTask,
    executed: &BTreeSet<&str>,
    catalog: &ToolCatalog,
) -> Result<CrateObligation, OrchestratorError> {
    let argv = crate::vectors::task_argv(task, catalog)?;
    let toolchain_inputs = crate::internal_plan::identities::toolchain_inputs_for(task, catalog)?;
    let toolchain = velnor_actions_contract::cachekey::toolchain_id(&toolchain_inputs)?;
    let digest = crate::internal::plan_obligation::task_digest(&task.task_id, &argv, &toolchain)?;
    let matrix_id = matrix_id_for_task_group(&task.stack_id, &task.task_id)?;
    let matrix_key = matrix_key_for_id(&matrix_id)?;
    Ok(CrateObligation {
        task_id: task.task_id.clone(),
        configuration: task.configuration.clone(),
        kind: task.task_kind.clone(),
        step_name: step_name_for(&task.task_kind, &task.task_id),
        gated_by: gates_for(task, executed),
        matrix_key,
        task_digest: digest,
        toolchain_inputs,
        run: argv,
    })
}

/// Sorted same-crate gates: quality gates plus data producers.
///
/// Gates naming skipped tasks (test-less doctests) are vacuous: the
/// kind order still sequences the survivors, and dangling references
/// would fail the strictly-earlier validation.
fn gates_for(task: &ProposedTask, executed: &BTreeSet<&str>) -> Vec<String> {
    let mut gates: Vec<String> = task
        .gated_by
        .iter()
        .chain(task.depends_on.iter())
        .filter(|gate| executed.contains(gate.as_str()))
        .cloned()
        .collect();
    gates.sort();
    gates.dedup();
    gates
}

/// Render one validated crate model to its fixed IR job.
///
/// P08 order: helper staging, plan download (report identities bind
/// the plan), restore the exact shared Cargo sources subset, the
/// per-root provider restore on opentofu roles, then MBX objects,
/// then probe-and-fetch, then report-wrapped obligations, then one
/// always-on crate-report upload carrying every entry. Readers never
/// save. Rust setup (components, restore, fetch) emits only for rust
/// roles; pure-tofu roles carry the opentofu driver with no Rust
/// setup, mixed roles the union.
#[cfg(test)]
#[path = "crate_jobs_test_modules.rs"]
mod test_modules;
#[cfg(test)]
pub(crate) use test_modules::crate_jobs_tests;

//! Crate-job construction: one job per crate with parallel check steps.
//!
//! Every Rust package/configuration owns exactly one job (a hard decision in
//! `docs/hard-decisions.md`): checkout, pinned tools, lockful sources and the
//! MBX target cache are prepared once, the test executables are built once,
//! and then Format, Clippy, Documentation, test execution and doctests run as
//! concurrent GitHub steps joined by one `wait-all`. A failed check fails the
//! job at that join; the always-on report upload still runs afterwards.
//! Clippy and documentation compile in their own Cargo target lanes so the
//! target-directory lock never serializes test execution behind them.
//! Same-job data producers (the Nextest build) are sequential obligations
//! strictly before the parallel group in the validated
//! [`CrateJob`](velnor_actions_contract::CrateJob). Gate owners in other jobs
//! become exact job dependencies. `OpenTofu` retains one job per root, its
//! sequential obligations and its existing bounded lane staging.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CrateJob, CrateObligation, Job, JobTimeout, ProposedTask, Stack, Step, VelnorConfig,
    WorkflowPolicy, crate_display_name, tofu_display_name,
};
#[cfg(test)]
use velnor_actions_mise::PinnedTool;
use velnor_actions_mise::{TaskCacheMode, ToolCatalog};
#[cfg(test)]
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::steps::CompileDriver as RenderDriver;

use crate::OrchestratorError;
use crate::crate_job_ids::{assign_group_ids, group_is_tofu, group_runnable};
use crate::discover::Discovery;
use crate::internal::internal;

#[path = "crate_jobs_stage.rs"]
mod stage;

#[path = "crate_jobs_gates.rs"]
mod gates;

#[path = "crate_jobs_obligations.rs"]
mod obligations;
use obligations::obligations_for;
#[cfg(test)]
use obligations::{gates_for, obligation_rank};

#[cfg(test)]
pub(crate) use stage::needs;
pub(crate) use stage::{is_mbx, is_nextest, is_opentofu, is_rust};

/// Built crate jobs plus their driver selections for MBX gating.
pub(crate) struct CrateBuild {
    /// `(job_id, job)` pairs in deterministic job-ID order.
    pub(crate) jobs: Vec<(String, Job)>,
    /// Render-driver selection per crate job ID.
    pub(crate) drivers: BTreeMap<String, RenderDriver>,
}

/// Build crate jobs with the repository's workflow and Rust-stack config.
/// # Errors
pub(crate) fn build_for_workflow(
    config: &VelnorConfig,
    label: &str,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    acquire: Option<&Step>,
    helper_version: &str,
) -> Result<CrateBuild, OrchestratorError> {
    build_crate_jobs(CrateJobInputs {
        label,
        policy: config.workflow.policy,
        discovery,
        catalog,
        fetch_roots,
        acquire,
        max_parallel_jobs: config.workflow.max_parallel_jobs,
        timeout: JobTimeout::new(config.workflow.crate_timeout_minutes)?,
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
    /// Literal timeout for every emitted crate job.
    pub(crate) timeout: JobTimeout,
    pub(crate) helper_version: &'a str,
    pub(crate) cache_policy: velnor_actions_contract::PullRequestCachePolicy,
}

/// Build one ordered IR job for each runnable package/configuration.
///
/// Tasks without test targets carry no command and stay out; the
/// package-less workspace Format scope belongs to the plan job, so its
/// tasks stay out too. Same-job data producers run first; the remaining
/// obligations keep the Format, Clippy, tests, doctests, docs order (shards
/// by task ID) and, in Rust jobs, run as one parallel group. Gates naming
/// other runnable owners become job dependencies. The acquire step stages
/// the helper every report wrapper invokes; callers without one (pre-seed,
/// lock attach) provision separately.
///
/// # Errors
///
/// Returns contract, render-context, or tool-request errors.
pub(crate) fn build_crate_jobs(input: CrateJobInputs<'_>) -> Result<CrateBuild, OrchestratorError> {
    let CrateJobInputs {
        discovery,
        catalog,
        max_parallel_jobs,
        ..
    } = input;
    let grouped = group_runnable(&discovery.proposals)?;
    let assigned = assign_group_ids(&grouped);
    let mut jobs = Vec::with_capacity(grouped.len());
    let mut drivers = BTreeMap::new();
    let mut tofu_ids = Vec::new();
    for (key, tasks) in &grouped {
        let first = tasks.first().ok_or_else(|| internal("crate_empty"))?;
        let job_id = assigned
            .get(key)
            .ok_or_else(|| internal("crate_job_id_missing"))?
            .clone();
        let manifest = first.identity.unit_path.clone();
        let display = if group_is_tofu(tasks) {
            tofu_display_name(&first.display_name)
        } else {
            crate_display_name(&first.display_name, &manifest, &key.configuration)
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
            job_id: job_id.clone(),
            display_name: display,
            package_name: first.display_name.clone(),
            package_id: key.package_id.clone(),
            manifest,
            configuration: key.configuration.clone(),
            obligations: obligations_for(tasks, catalog)?,
        };
        model.validate()?;
        let needs = gates::external_needs(tasks, &discovery.proposals, &job_id)?;
        let job = render_job(
            &input,
            &model,
            use_rust,
            use_mbx,
            use_nextest,
            use_opentofu,
            needs,
        )?;
        drivers.insert(job_id.clone(), driver);
        if use_opentofu {
            tofu_ids.push(job_id.clone());
        }
        jobs.push((job_id, job));
    }
    stage::stage_tofu_root_jobs(&mut jobs, &tofu_ids, max_parallel_jobs);
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(CrateBuild { jobs, drivers })
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
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "one call site threads job scope plus driver selection"
)]
fn render_job(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
    needs: Vec<String>,
) -> Result<Job, OrchestratorError> {
    let &CrateJobInputs {
        label,
        policy,
        catalog,
        fetch_roots,
        ..
    } = input;
    let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
    steps.extend(input.acquire.cloned());
    steps.push(crate::matrix_step::download_plan_step()?);
    let suite: Option<crate::matrix_step::CrateSuite> =
        crate::matrix_step::suite_for_package(&model.package_name);
    let needs_validators = crate::matrix_step::crate_needs_generate_validators(policy, suite);
    steps.push(crate::matrix_step::prepare_crate_tools_step(
        catalog,
        use_rust,
        use_nextest,
        crate::matrix_step::prepare_install_opentofu(policy, suite, use_opentofu),
        needs_validators,
    )?);
    if use_rust {
        steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    }
    steps.extend(crate::source_cache::restore_step_for_crate(
        label,
        catalog,
        fetch_roots,
        use_rust,
    )?);
    if use_opentofu {
        let root = crate::tofu_cache::tofu_root_for_obligations(&model.obligations)?;
        steps.extend(crate::tofu_cache::provider_cache_step_for_tofu_root(
            label, catalog, &root,
        )?);
    }
    if use_mbx {
        let root = crate::source_prep::workspace_root_for_manifest(&model.manifest, fetch_roots);
        let working_dir = (!root.is_empty()).then_some(root);
        steps.extend(crate::mbx_preflight::steps_for_catalog(
            catalog,
            input.cache_policy,
            working_dir,
        )?);
    }
    if use_rust {
        let scoped_roots = crate::source_prep::crate_fetch_roots(
            &model.manifest,
            fetch_roots,
            policy == WorkflowPolicy::VelnorRepositoryV1 && needs_validators,
        );
        steps.extend(crate::source_prep::fetch_steps_for_crate(
            catalog,
            &scoped_roots,
        )?);
    }
    steps.extend(crate::workflow::wire_w1::maybe_task_cache_steps(
        None,
        TaskCacheMode::Off,
        "",
    )?);
    steps.extend(obligation_steps(input, model, use_opentofu)?);
    steps.push(crate::matrix_step::crate_upload_step(&model.job_id)?);
    Ok(Job {
        display_name: model.display_name.clone(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: input.timeout,
        needs,
        condition: None,
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    })
}

/// Report-wrapped obligation steps in validated order.
///
/// In a Rust job every obligation except the sequential producers (the
/// Nextest build) is a member of one parallel group, when at least two
/// exist. No member may gate on another member. Clippy and documentation
/// members compile in their own target lane below the workspace target.
fn obligation_steps(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    use_opentofu: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let sequential = |obligation: &CrateObligation| {
        velnor_actions_rust::runs_before_parallel_checks(&obligation.kind)
    };
    let concurrent = !use_opentofu
        && model
            .obligations
            .iter()
            .filter(|obligation| !sequential(obligation))
            .count()
            >= 2;
    let members: BTreeSet<&str> = model
        .obligations
        .iter()
        .filter(|obligation| concurrent && !sequential(obligation))
        .map(|obligation| obligation.task_id.as_str())
        .collect();
    let target_root =
        crate::source_prep::workspace_root_for_manifest(&model.manifest, input.fetch_roots);
    let mut steps = Vec::with_capacity(model.obligations.len());
    for (index, obligation) in model.obligations.iter().enumerate() {
        let parallel = members.contains(obligation.task_id.as_str());
        if !parallel && steps.iter().any(|step: &Step| step.parallel) {
            return Err(internal("crate_parallel_group_split"));
        }
        if parallel
            && obligation
                .gated_by
                .iter()
                .any(|gate| members.contains(gate.as_str()))
        {
            return Err(internal("crate_parallel_member_gate"));
        }
        let lane = parallel
            .then(|| velnor_actions_rust::concurrent_target_lane(&obligation.kind))
            .flatten()
            .map(|lane| lane_target_dir(target_root, lane));
        let downstream: Vec<String> = model.obligations[index + 1..]
            .iter()
            .map(|later| later.task_id.clone())
            .collect();
        // The first obligation declares the root job's concurrency cap;
        // the renderer turns the marker into `strategy.max-parallel`.
        let cap = (index == 0 && use_opentofu).then_some(input.max_parallel_jobs);
        let mut step = crate::matrix_step::obligation_step(
            obligation,
            input.catalog,
            &downstream,
            cap,
            input.helper_version,
            lane.as_deref(),
        )?;
        step.parallel = parallel;
        steps.push(step);
    }
    Ok(steps)
}

/// Workspace-relative Cargo target lane directory for one concurrent check.
fn lane_target_dir(workspace_root: &str, lane: &str) -> String {
    if workspace_root.is_empty() {
        format!("target/{lane}")
    } else {
        format!("{workspace_root}/target/{lane}")
    }
}

#[cfg(test)]
#[path = "crate_jobs_tests.rs"]
mod crate_jobs_tests;

#[cfg(test)]
#[path = "crate_jobs_display_tests.rs"]
mod crate_jobs_display_tests;

#[cfg(test)]
#[path = "crate_jobs_tofu_cache_tests.rs"]
mod crate_jobs_tofu_cache_tests;

#[cfg(test)]
#[path = "crate_jobs_tofu_tests.rs"]
mod crate_jobs_tofu_tests;

#[cfg(test)]
#[path = "crate_jobs_upload_tests.rs"]
mod crate_jobs_upload_tests;

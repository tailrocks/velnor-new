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

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CrateJob, Job, JobTimeout, ProposedTask, RustCrateConfig, Stack, Step, VelnorConfig,
    WorkflowPolicy, crate_display_name, tofu_display_name,
};
use velnor_actions_mise::{TaskCacheMode, ToolCatalog};
use velnor_actions_workflow_renderer::steps::CompileDriver as RenderDriver;
#[cfg(test)]
use {velnor_actions_mise::PinnedTool, velnor_actions_workflow_renderer::render::PLAN_JOB_ID};

use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;
use crate::crate_job_ids::{JobOwners, group_is_tofu, group_runnable};
use crate::discover::Discovery;
use crate::internal::internal;

#[path = "dependency_seed.rs"]
pub(crate) mod dependency_seed;

#[path = "crate_jobs_stage.rs"]
mod stage;

#[path = "crate_jobs_gates.rs"]
mod gates;

#[path = "crate_jobs_build_jobs.rs"]
mod build_jobs;
use build_jobs::reject_unknown_crates;

#[path = "crate_jobs_steps.rs"]
mod job_steps;
#[path = "crate_jobs_obligations.rs"]
mod obligations;
use job_steps::obligation_steps;
use obligations::obligations_for;
#[cfg(test)]
use obligations::{gates_for, obligation_rank};

#[cfg(test)]
pub(crate) use stage::needs;
pub(crate) use stage::{is_mbx, is_nextest, is_opentofu, is_rust};

/// Built crate jobs plus their driver selections for MBX gating.
pub(crate) struct CrateBuild {
    /// Dependency-seed jobs (cache decision 2): never part of `Required`.
    pub(crate) seeds: Vec<(String, Job)>,
    /// `(job_id, job)` pairs in deterministic job-ID order.
    pub(crate) jobs: Vec<(String, Job)>,
    /// Render-driver selection per crate job ID.
    pub(crate) drivers: BTreeMap<String, RenderDriver>,
}

/// Build crate jobs with the repository's workflow and Rust-stack config.
/// # Errors
#[expect(
    clippy::too_many_arguments,
    reason = "one call site; the private-source wiring joins the existing inputs"
)]
pub(crate) fn build_for_workflow(
    config: &VelnorConfig,
    label: &str,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    acquire: Option<&Step>,
    helper_version: &str,
    private: Option<&PrivateWiring>,
) -> Result<CrateBuild, OrchestratorError> {
    let no_crates = BTreeMap::new();
    let crates = config
        .stacks
        .rust
        .as_ref()
        .map_or(&no_crates, |rust| &rust.crates);
    reject_unknown_crates(crates, discovery)?;
    build_crate_jobs_with(
        CrateJobInputs {
            label,
            policy: config.workflow.policy,
            discovery,
            catalog,
            fetch_roots,
            acquire,
            max_parallel_jobs: config.workflow.max_parallel_jobs,
            timeout: JobTimeout::new(config.workflow.crate_timeout_minutes)?,
            helper_version,
            crates,
            seed_parallel_targets: config
                .stacks
                .rust
                .as_ref()
                .is_some_and(|rust| rust.seed_parallel_targets),
        },
        private,
    )
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
    /// Opt-in cold target initialization; never changes obligation ownership.
    pub(crate) seed_parallel_targets: bool,
    /// Per-package Rust options keyed by Cargo package name.
    pub(crate) crates: &'a BTreeMap<String, RustCrateConfig>,
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
#[cfg(test)]
pub(crate) fn build_crate_jobs(input: CrateJobInputs<'_>) -> Result<CrateBuild, OrchestratorError> {
    build_crate_jobs_with(input, None)
}

/// [`build_crate_jobs`] plus the private-source block (HD-7) for jobs whose
/// lock closure is non-empty.
///
/// # Errors
///
/// Returns contract, render-context, or tool-request errors.
pub(crate) fn build_crate_jobs_with(
    input: CrateJobInputs<'_>,
    private: Option<&PrivateWiring>,
) -> Result<CrateBuild, OrchestratorError> {
    let CrateJobInputs {
        discovery,
        catalog,
        max_parallel_jobs,
        ..
    } = input;
    let grouped = group_runnable(&discovery.proposals)?;
    let owners = JobOwners::from_grouped(&discovery.proposals, &grouped);
    let mut jobs = Vec::with_capacity(grouped.len());
    let mut drivers = BTreeMap::new();
    let mut tofu_ids = Vec::new();
    let mut seed_workspaces: BTreeMap<String, bool> = BTreeMap::new();
    for (key, tasks) in &grouped {
        let first = tasks.first().ok_or_else(|| internal("crate_empty"))?;
        let job_id = owners
            .group_id(key)
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
        let needs = gates::external_needs(tasks, &owners, &job_id)?;
        let job = render_job(
            &input,
            &model,
            use_rust,
            use_mbx,
            use_nextest,
            use_opentofu,
            needs,
            private,
        )?;
        if use_mbx {
            let root =
                crate::source_prep::workspace_root_for_manifest(&model.manifest, input.fetch_roots);
            *seed_workspaces.entry(root.to_owned()).or_default() |= use_nextest;
        }
        drivers.insert(job_id.clone(), driver);
        if use_opentofu {
            tofu_ids.push(job_id.clone());
        }
        jobs.push((job_id, job));
    }
    stage::stage_tofu_root_jobs(&mut jobs, &tofu_ids, max_parallel_jobs);
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    let workspaces: Vec<_> = seed_workspaces
        .into_iter()
        .map(|(root, nextest)| dependency_seed::SeedWorkspace { root, nextest })
        .collect();
    let seeds = dependency_seed::seed_jobs(&input, &workspaces, private)?;
    for (id, _) in &seeds {
        drivers.insert(id.clone(), RenderDriver::Mbx);
    }
    Ok(CrateBuild {
        seeds,
        jobs,
        drivers,
    })
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

/// Private-source block (HD-7) plus the Cargo source fetch of one Rust job.
fn rust_source_steps(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    needs_validators: bool,
    private: Option<&PrivateWiring>,
) -> Result<Vec<Step>, OrchestratorError> {
    let scoped_roots = crate::source_prep::crate_fetch_roots(
        &model.manifest,
        input.fetch_roots,
        input.policy == WorkflowPolicy::VelnorRepositoryV1 && needs_validators,
    );
    let all_features = model
        .obligations
        .iter()
        .any(|obligation| obligation.run.iter().any(|arg| arg == "--all-features"));
    let mut steps = match private {
        Some(private) => private.steps_for_roots(&scoped_roots)?,
        None => Vec::new(),
    };
    steps.extend(crate::source_prep::fetch_steps_for_crate(
        input.catalog,
        &scoped_roots,
        all_features,
    )?);
    Ok(steps)
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
    clippy::too_many_arguments,
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
    private: Option<&PrivateWiring>,
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
        steps.push(crate::workflow::verify_rust_manifest_step(catalog)?);
    }
    steps.extend(crate::source_cache::restore_step_for_crate(
        fetch_roots,
        use_rust,
    )?);
    if use_opentofu {
        let root = crate::tofu_cache::tofu_root_for_obligations(&model.obligations)?;
        let roots = crate::tofu_cache::tofu_cache_roots(input.discovery)?;
        steps.extend(crate::tofu_cache::provider_cache_step_for_tofu_root(
            &root, &roots,
        )?);
    }
    if use_mbx {
        let root = crate::source_prep::workspace_root_for_manifest(&model.manifest, fetch_roots);
        let working_dir = (!root.is_empty()).then_some(root);
        steps.extend(crate::mbx_preflight::steps_for_catalog(
            catalog,
            velnor_actions_workflow_renderer::steps::MbxTransport::SeedRead,
            working_dir,
        )?);
    }
    if use_rust {
        steps.extend(rust_source_steps(input, model, needs_validators, private)?);
    }
    steps.extend(crate::workflow::wire_w1::maybe_task_cache_steps(
        None,
        TaskCacheMode::Off,
        "",
    )?);
    steps.extend(obligation_steps(input, model, use_opentofu)?);
    steps.push(crate::matrix_step::crate_upload_step(&model.job_id)?);
    Ok(Job {
        services: Vec::new(),
        display_name: model.display_name.clone(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: input.timeout,
        needs,
        condition: (use_rust && !use_opentofu)
            .then(|| velnor_actions_workflow_renderer::not_affected_gate(&model.job_id)),
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    })
}

#[cfg(test)]
#[path = "crate_jobs_tests.rs"]
pub(crate) mod crate_jobs_tests;

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

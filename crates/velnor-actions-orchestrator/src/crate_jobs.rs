//! Crate-job construction: closed capacity phases with exact obligation owners.
//!
//! The P05 migration removed arbitrary per-task matrix fan-out by grouping
//! obligations into ordered package/configuration jobs. Cold Jackin CI later
//! demonstrated that serial Clippy, test compilation and documentation can
//! exhaust the default five-minute budget before all obligations execute.
//! Rust now groups by package/configuration and the adapter's fixed lint,
//! tests and docs phases. Test builds and consumers remain on one runner;
//! logical task identities, argument vectors and report authorities stay intact.
//! Same-phase gates reference strictly earlier obligations in the validated
//! [`CrateJob`](velnor_actions_contract::CrateJob). External gate owners become
//! exact job dependencies, so splitting execution never discards quality gates.
//! Each job retains checkout, pinned tools, lockful sources, stack-owned cache
//! preparation, ordered report-wrapped obligations and its own report upload.
//! `OpenTofu` retains one job per root and its existing bounded lane staging.

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
use crate::crate_job_ids::{assign_group_ids, group_is_tofu, group_runnable};
use crate::discover::Discovery;
use crate::internal::internal;
use crate::matrix_step::step_name_for;
use crate::obligation_order::obligation_order_key;

#[path = "crate_jobs_stage.rs"]
mod stage;

#[path = "crate_jobs_phase.rs"]
mod phase;

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

/// Build ordered IR jobs for each nonempty package/configuration capacity phase.
///
/// Tasks without test targets carry no command and stay out; the
/// package-less workspace Format scope belongs to the plan job, so its
/// tasks stay out too. Obligations order Format, Clippy, build, tests,
/// doctests, docs (shards by task ID); gates keep same-phase edges
/// locally and lift other runnable owners into job dependencies. The
/// acquire step stages the helper every report wrapper invokes;
/// callers without one (pre-seed, lock attach) provision separately.
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
            let base = crate_display_name(&first.display_name, &manifest, &key.configuration);
            match key.phase {
                Some(phase) => format!("{base} / {}", phase.display_name()),
                None => base,
            }
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
        let needs = phase::external_needs(tasks, &discovery.proposals, &job_id)?;
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

/// One obligation: identities, same-phase ordered gates, fixed argv.
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
        kind: task.task_kind.clone(),
        step_name: step_name_for(&task.task_kind, &task.task_id),
        gated_by: gates_for(task, executed),
        matrix_key,
        task_digest: digest,
        toolchain_inputs,
        run: argv,
    })
}

/// Sorted same-phase gates: quality gates plus data producers.
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
fn obligation_steps(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    use_opentofu: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::with_capacity(model.obligations.len());
    for (index, obligation) in model.obligations.iter().enumerate() {
        let downstream: Vec<String> = model.obligations[index + 1..]
            .iter()
            .map(|later| later.task_id.clone())
            .collect();
        // The first obligation declares the root job's concurrency cap;
        // the renderer turns the marker into `strategy.max-parallel`.
        let cap = (index == 0 && use_opentofu).then_some(input.max_parallel_jobs);
        steps.push(crate::matrix_step::obligation_step(
            obligation,
            input.catalog,
            &downstream,
            cap,
            input.helper_version,
        )?);
    }
    Ok(steps)
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

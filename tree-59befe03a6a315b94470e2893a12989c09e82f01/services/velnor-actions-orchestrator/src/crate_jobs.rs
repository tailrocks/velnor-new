//! Crate-job construction: one ordered IR job per crate (P05).
//!
//! Root cause (P05): logical obligations were equated with execution
//! jobs, so every task fanned out into matrix legs behind one shared
//! template. The structural fix groups runnable obligations by
//! `(package, configuration)` into one ordered [`CrateJob`](velnor_actions_contract_workflow::CrateJob)
//! each, validates the grouping through the contract model (stable
//! unbranded IDs, gates referencing strictly earlier obligations),
//! then renders each group to a fixed IR job: checkout, pinned tools,
//! components, lockful sources, the per-root provider restore on
//! opentofu crates, the MBX objects restore on MBX crates, and one
//! shell step per obligation in gate order.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Stack, matrix_id_for_task_group, matrix_key_for_id};
use velnor_actions_contract_config::{VelnorConfig, WorkflowPolicy};
use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_contract_workflow::{
    CrateJob, CrateObligation, Job, JobTimeout, Step, crate_display_name, tofu_display_name,
};
use velnor_actions_mise::{PinnedTool, TaskCacheMode, ToolCatalog};
use velnor_actions_workflow_cache::cache_steps::CompileDriver as RenderDriver;
use velnor_actions_workflow_jobs::context::PLAN_JOB_ID;

use crate::OrchestratorError;
use crate::crate_job_ids::{assign_group_ids, group_is_tofu, group_runnable};
use crate::discover::Discovery;
use crate::internal::internal;
use crate::matrix_step::step_name_for;
use crate::obligation_order::obligation_order_key;

mod stage;

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
) -> Result<CrateBuild, OrchestratorError> {
    build_crate_jobs(
        label,
        config.workflow.policy,
        discovery,
        catalog,
        fetch_roots,
        acquire,
        config.workflow.max_parallel_jobs,
    )
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
pub(crate) fn build_crate_jobs(
    label: &str,
    policy: WorkflowPolicy,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    acquire: Option<&Step>,
    max_parallel_jobs: u32,
) -> Result<CrateBuild, OrchestratorError> {
    let grouped = group_runnable(&discovery.proposals);
    let assigned = assign_group_ids(&grouped);
    let mut jobs = Vec::with_capacity(grouped.len());
    let mut drivers = BTreeMap::new();
    let mut tofu_ids = Vec::new();
    for ((package_id, configuration), tasks) in &grouped {
        let first = tasks.first().ok_or_else(|| internal("crate_empty"))?;
        let key = (package_id.clone(), configuration.clone());
        let job_id = assigned
            .get(&key)
            .ok_or_else(|| internal("crate_job_id_missing"))?
            .clone();
        let manifest = first.identity.unit_path.clone();
        let display = if group_is_tofu(tasks) {
            tofu_display_name(&first.display_name)
        } else {
            crate_display_name(&first.display_name, &manifest, configuration)
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
            package_id: package_id.clone(),
            manifest,
            configuration: configuration.clone(),
            obligations: obligations_for(tasks, catalog)?,
        };
        model.validate()?;
        let repo_has_mbx = crate::workflow::plan_uses_mbx(discovery);
        let job = render_job(
            label,
            policy,
            &model,
            catalog,
            fetch_roots,
            use_rust,
            use_mbx,
            use_nextest,
            use_opentofu,
            repo_has_mbx,
            acquire,
            max_parallel_jobs,
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

/// One obligation: identities, same-crate gates, fixed argv.
fn obligation_for(
    task: &ProposedTask,
    executed: &BTreeSet<&str>,
    catalog: &ToolCatalog,
) -> Result<CrateObligation, OrchestratorError> {
    let argv = crate::vectors::task_argv(task, catalog)?;
    let toolchain = crate::internal_plan::toolchain_id(task, catalog)?;
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
/// the plan), restore shared sources (or Cargo-only registry), the
/// per-root provider restore on opentofu roles, then MBX objects,
/// then probe-and-fetch, then report-wrapped obligations, then one
/// always-on crate-report upload carrying every entry. Readers never
/// save. Rust setup (components, restore, fetch) emits only for rust
/// roles; pure-tofu roles carry the opentofu driver with no Rust
/// setup, mixed roles the union.
#[expect(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "one call site threads job scope plus driver selection"
)]
fn render_job(
    label: &str,
    policy: WorkflowPolicy,
    model: &CrateJob,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
    repo_has_mbx: bool,
    acquire: Option<&Step>,
    max_parallel_jobs: u32,
) -> Result<Job, OrchestratorError> {
    let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
    steps.extend(acquire.cloned());
    steps.push(crate::matrix_step::download_plan_step()?);
    let needs_validators =
        crate::matrix_step::crate_needs_generate_validators(policy, &model.package_name);
    steps.push(crate::matrix_step::prepare_crate_tools_step(
        catalog,
        use_rust,
        use_nextest,
        crate::matrix_step::prepare_install_opentofu(policy, &model.package_name, use_opentofu),
        needs_validators,
    )?);
    if use_rust {
        steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    }
    steps.extend(restore_step_for_crate(
        label,
        catalog,
        fetch_roots,
        use_rust,
        use_mbx,
        repo_has_mbx,
    )?);
    if use_opentofu {
        let root = crate::tofu_cache::tofu_root_for_obligations(&model.obligations)?;
        steps.extend(crate::tofu_cache::provider_cache_step_for_tofu_root(
            label, catalog, &root,
        )?);
    }
    if use_mbx {
        steps.extend(crate::mbx_preflight::steps_for_catalog(catalog)?);
    }
    if use_rust {
        steps.extend(crate::source_prep::fetch_steps_for_crate(
            catalog,
            fetch_roots,
        )?);
    }
    steps.extend(crate::workflow::wire_w1::maybe_task_cache_steps(
        None,
        TaskCacheMode::Off,
        "",
    )?);
    for (index, obligation) in model.obligations.iter().enumerate() {
        let downstream: Vec<String> = model.obligations[index + 1..]
            .iter()
            .map(|later| later.task_id.clone())
            .collect();
        // The first obligation declares the root job's concurrency cap;
        // the renderer turns the marker into `strategy.max-parallel`.
        let cap = (index == 0 && use_opentofu).then_some(max_parallel_jobs);
        steps.push(crate::matrix_step::obligation_step(
            obligation,
            catalog,
            &downstream,
            cap,
        )?);
    }
    steps.push(crate::matrix_step::crate_upload_step(&model.job_id)?);
    Ok(Job {
        display_name: model.display_name.clone(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps,
    })
}

/// Restore step for one crate: shared sources, or Cargo-only registry.
///
/// Lockless emits nothing, and tofu roles restore providers through
/// the separate provider-cache step (never here). Cargo-only repos
/// (no MBX anywhere) restore via pinned `rust-cache` (read-only);
/// every other lockful crate restores the shared `actions/cache`
/// snapshot (read-only, never saves the shared key).
fn restore_step_for_crate(
    label: &str,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    use_rust: bool,
    use_mbx: bool,
    repo_has_mbx: bool,
) -> Result<Option<Step>, OrchestratorError> {
    if !use_rust || fetch_roots.is_empty() {
        return Ok(None);
    }
    let target = velnor_actions_contract_release::ReleaseTarget::for_runner_label(label)
        .map(velnor_actions_contract_release::ReleaseTarget::triple)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("bad_label:{label}"),
        })?;
    let rust = catalog.version(PinnedTool::Rust);
    if !use_mbx && !repo_has_mbx {
        let shared = format!(
            "{}-{target}-{rust}",
            crate::source_cache::RUST_CACHE_SHARED_PREFIX
        );
        return crate::source_cache::rust_cache_step(&shared, false).map(Some);
    }
    let key = crate::source_cache::sources_cache_key(target, rust, fetch_roots)?;
    let prefix = crate::source_cache::sources_restore_prefix(&key);
    crate::source_cache::sources_restore_step(&key, &[prefix]).map(Some)
}

#[cfg(test)]
mod tests;

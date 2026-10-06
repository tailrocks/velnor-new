//! Crate-job construction: one ordered IR job per crate (P05).
//!
//! Root cause (P05): logical obligations were equated with execution
//! jobs, so every task fanned out into matrix legs behind one shared
//! template. The structural fix groups runnable obligations by
//! `(package, configuration)` into one ordered [`CrateJob`](velnor_actions_contract::CrateJob)
//! each, validates the grouping through the contract model (stable
//! unbranded IDs, gates referencing strictly earlier obligations),
//! then renders each group to a fixed IR job: checkout, pinned tools,
//! components, lockful sources, the per-root provider restore on
//! opentofu crates, the MBX objects restore on MBX crates, and one
//! shell step per obligation in gate order.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CrateJob, CrateObligation, Job, ProposedTask, Stack, Step, WorkflowPolicy, crate_display_name,
    matrix_id_for_task_group, matrix_key_for_id, tofu_display_name,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::task_kind_rank;
use velnor_actions_workflow_renderer::steps::CompilerDriver;

use crate::OrchestratorError;
use crate::crate_job_ids::{assign_group_ids, group_is_tofu, group_runnable};
use crate::discover::Discovery;
use crate::internal::internal;
use crate::matrix_step::step_name_for;

#[path = "crate_jobs_render.rs"]
mod render;
use render::render_job;

#[path = "crate_obligation_helpers.rs"]
pub(crate) mod helpers;

#[path = "crate_jobs_stage.rs"]
mod stage;

#[cfg(test)]
pub(crate) use stage::needs;
pub(crate) use stage::{is_mbx, is_nextest, is_opentofu, is_rust};

/// Built crate jobs plus their driver selections for MBX gating.
pub(crate) struct CrateBuild {
    /// `(job_id, job)` pairs in deterministic job-ID order.
    pub(crate) jobs: Vec<(String, Job)>,
    /// Render-driver selection per crate job ID.
    pub(crate) drivers: BTreeMap<String, CompilerDriver>,
    /// Exact source records retained from obligation compilation.
    pub(crate) helper_records: Vec<velnor_actions_contract::CompiledSourceHelper>,
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
#[expect(
    clippy::too_many_arguments,
    reason = "one call site threads job scope plus the concurrency cap"
)]
pub(crate) fn build_crate_jobs(
    label: &str,
    policy: WorkflowPolicy,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    custom_tasks: &[String],
    acquire: Option<&Step>,
    max_parallel_jobs: u32,
) -> Result<CrateBuild, OrchestratorError> {
    let grouped = group_runnable(&discovery.proposals);
    let assigned = assign_group_ids(&grouped);
    // Reject a non-empty allowlist before the loop: with zero runnable
    // tasks the loop body (and its rejection) never runs, so the
    // allowlist would be silently ignored instead of failing closed.
    let custom_steps = crate::vectors::custom_task_steps(custom_tasks, catalog)?;
    let mut jobs = Vec::with_capacity(grouped.len());
    let mut drivers = BTreeMap::new();
    let mut tofu_ids = Vec::new();
    let mut helper_records = Vec::new();
    for ((package_id, configuration), tasks) in &grouped {
        let first = tasks.first().ok_or_else(|| internal("crate_empty"))?;
        let runner_label = crate::workloads::runner_for_task(first, label);
        let scoped_catalog = crate::workloads::catalog_for_configuration(catalog, configuration)
            .map_err(|error| OrchestratorError::Contract {
                problem: error.to_string(),
            })?;
        let catalog = &scoped_catalog;
        let key = (package_id.clone(), configuration.clone());
        let job_id = assigned
            .get(&key)
            .ok_or_else(|| internal("crate_job_id_missing"))?
            .clone();
        let manifest = first.identity.unit_path.clone();
        let display = group_display(tasks, first, configuration, &manifest);
        let use_rust = tasks.iter().any(|task| is_rust(task));
        let use_mbx = tasks.iter().any(|task| is_mbx(task));
        let use_nextest = tasks.iter().any(|task| is_nextest(task));
        let use_opentofu = tasks.iter().any(|task| is_opentofu(task));
        let driver = driver_for(use_mbx);
        let (obligations, bindings) = compile_obligations(tasks, catalog, runner_label)?;
        let model = CrateJob {
            job_id: job_id.clone(),
            display_name: display,
            package_name: first.display_name.clone(),
            package_id: package_id.clone(),
            manifest,
            configuration: configuration.clone(),
            obligations,
        };
        model.validate()?;
        let selected_roots =
            crate::source_prep::selected_fetch_roots(discovery, tasks, fetch_roots);
        let _desktop_profile = crate::workloads::identity_recipe::desktop_profile(tasks)?;
        let provider_descriptor = crate::tofu_cache_source::descriptor_for_tasks(tasks);
        let (mut job, records) = render_job(
            label,
            policy,
            &model,
            catalog,
            &selected_roots,
            use_rust,
            use_mbx,
            use_nextest,
            use_opentofu,
            acquire,
            max_parallel_jobs,
            provider_descriptor.as_ref(),
            &bindings,
        )?;
        helper_records.extend(records);
        // Allowlisted custom tasks run after the fixed obligations; the
        // pre-loop rejection above guarantees this is empty today.
        job.steps.extend(custom_steps.iter().cloned());
        drivers.insert(job_id.clone(), driver);
        if use_opentofu {
            tofu_ids.push(job_id.clone());
        }
        jobs.push((job_id, job));
    }
    stage::stage_tofu_root_jobs(&mut jobs, &tofu_ids, max_parallel_jobs);
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(CrateBuild {
        jobs,
        drivers,
        helper_records,
    })
}

fn group_display(
    tasks: &[&ProposedTask],
    first: &ProposedTask,
    configuration: &str,
    manifest: &str,
) -> String {
    if crate::crate_job_ids::group_is_workload(tasks) {
        velnor_actions_contract::workload_display_name(&first.display_name, configuration)
    } else if group_is_tofu(tasks) {
        tofu_display_name(&first.display_name)
    } else {
        crate_display_name(&first.display_name, manifest, configuration)
    }
}

fn driver_for(use_mbx: bool) -> CompilerDriver {
    if use_mbx {
        CompilerDriver::Mbx
    } else {
        CompilerDriver::Cargo
    }
}

/// True when the task becomes a crate-job obligation.
///
/// Test-less tasks carry no command and package-less workspace tasks
/// belong to the plan job, so neither is emitted as an obligation.
/// Shared with `plan` so its obligation list matches emission exactly.
pub(crate) fn is_runnable(task: &ProposedTask) -> bool {
    !task.no_targets && !task.identity.unit_id.is_empty()
}

/// Obligation order rank for one task, dispatched by stack.
fn obligation_rank(task: &ProposedTask) -> u32 {
    obligation_kind_rank(&task.stack_id, &task.task_kind)
}

/// Generator sequencing authority shared by planned report validation.
pub(crate) fn obligation_kind_rank(stack: &str, kind: &str) -> u32 {
    match Stack::from_id(stack) {
        Some(Stack::Tofu) => velnor_actions_tofu::task_kind_rank(kind),
        Some(Stack::Workload) => crate::workloads::rank(kind),
        _ => task_kind_rank(kind),
    }
}

/// Ordered validated obligations for one crate's tasks.
pub(crate) fn compile_obligations(
    tasks: &[&ProposedTask],
    catalog: &ToolCatalog,
    label: &str,
) -> Result<(Vec<CrateObligation>, helpers::SourceBindings), OrchestratorError> {
    let executed: BTreeSet<&str> = tasks.iter().map(|task| task.task_id.as_str()).collect();
    let mut ordered = tasks.to_vec();
    ordered.sort_by(|left, right| {
        (obligation_rank(left), &left.task_id).cmp(&(obligation_rank(right), &right.task_id))
    });
    let mut obligations = Vec::with_capacity(ordered.len());
    let mut bindings = helpers::SourceBindings::default();
    for task in ordered {
        let compiled = obligation_for(task, &executed, catalog, label)?;
        if let Some(binding) = compiled.native {
            bindings.insert_source_binding(&compiled.obligation, binding)?;
        }
        if let Some(recipe) = compiled.compiler {
            bindings.insert_compiler_recipe(&compiled.obligation, recipe)?;
        }
        obligations.push(compiled.obligation);
    }
    Ok((obligations, bindings))
}

struct CompiledObligation {
    obligation: CrateObligation,
    native: Option<crate::helper_obligation_binding::ProposalHelperBinding>,
    compiler: Option<crate::rust_report_wrapper::RustReportWrapper>,
}

/// One original proposal supplies its execution identity and owned recipe.
fn obligation_for(
    task: &ProposedTask,
    executed: &BTreeSet<&str>,
    catalog: &ToolCatalog,
    label: &str,
) -> Result<CompiledObligation, OrchestratorError> {
    let compiler =
        crate::rust_report_wrapper::RustReportWrapper::from_proposal(task, catalog, label)?;
    let helper = crate::helper_obligation_binding::binding_for_proposal(
        task,
        catalog,
        env!("CARGO_PKG_VERSION"),
        label,
    )?;
    if let Some(binding) = &helper {
        binding.record.validate_binding()?;
    }
    let (argv, digest) = if let Some(recipe) = &compiler {
        (recipe.argv().to_vec(), recipe.task_digest().to_owned())
    } else {
        let argv = crate::vectors::task_argv_for_runner(task, catalog, label)?;
        let toolchain = crate::internal_plan::toolchain_id_for_runner(task, catalog, label)?;
        let digest = crate::internal::plan_obligation::task_digest(
            &task.task_id,
            &argv,
            &toolchain,
            helper.as_ref().map(|binding| &binding.descriptor),
            helper
                .as_ref()
                .and_then(|binding| binding.native_recipe.as_ref()),
        )?;
        (argv, digest)
    };
    let matrix_id = matrix_id_for_task_group(&task.stack_id, &task.task_id)?;
    let matrix_key = matrix_key_for_id(&matrix_id)?;
    Ok(CompiledObligation {
        obligation: CrateObligation {
            task_id: task.task_id.clone(),
            kind: task.task_kind.clone(),
            step_name: step_name_for(&task.task_kind, &task.task_id),
            gated_by: gates_for(task, executed),
            matrix_key,
            task_digest: digest,
            run: argv,
        },
        native: helper,
        compiler,
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

#[cfg(test)]
#[path = "crate_jobs_source_helper_tests.rs"]
mod source_helper_tests;

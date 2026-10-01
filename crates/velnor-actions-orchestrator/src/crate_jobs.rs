//! Crate-job construction: one ordered IR job per crate (P05).
//!
//! Root cause (P05): logical obligations were equated with execution
//! jobs, so every task fanned out into matrix legs behind one shared
//! template. The structural fix groups runnable obligations by
//! `(package, configuration)` into one ordered [`CrateJob`](velnor_actions_contract::CrateJob)
//! each, validates the grouping through the contract model (stable
//! unbranded IDs, gates referencing strictly earlier obligations),
//! then renders each group to a fixed IR job: checkout, pinned tools,
//! components, lockful sources, the MBX objects restore on MBX crates,
//! and one shell step per obligation in gate order.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_actionlint::{
    PinnedActionRef,
    actions::{MR_BOXINGTON_ACTION_SHA, MR_BOXINGTON_ACTION_VERSION},
};
use velnor_actions_contract::{
    CrateJob, CrateObligation, Job, Step, assign_crate_job_ids, crate_display_name,
    matrix_id_for_task_group, matrix_key_for_id,
};
use velnor_actions_mise::{PinnedTool, TaskCacheMode, ToolCatalog};
use velnor_actions_rust::{CompileDriver as RustDriver, STACK_ID, TaskGroup, TaskKind, TestRunner};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::steps::{CompileDriver as RenderDriver, mbx_step_for_driver};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::internal::internal;
use crate::matrix_step::step_name_for;

/// Built crate jobs plus their driver selections for MBX gating.
pub(crate) struct CrateBuild {
    /// `(job_id, job)` pairs in deterministic job-ID order.
    pub(crate) jobs: Vec<(String, Job)>,
    /// Render-driver selection per crate job ID.
    pub(crate) drivers: BTreeMap<String, RenderDriver>,
}

/// Build one ordered IR job per runnable crate from discovery groups.
///
/// Groups without test targets carry no command and stay out; the
/// package-less workspace Format scope belongs to the plan job, so its
/// groups stay out too. Obligations order Format, Clippy, build, tests,
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
    discovery: &Discovery,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    custom_tasks: &[String],
    acquire: Option<&Step>,
) -> Result<CrateBuild, OrchestratorError> {
    let grouped = group_runnable(&discovery.task_groups);
    let assigned = assign_crate_job_ids(&id_inputs(&grouped));
    let mut jobs = Vec::with_capacity(grouped.len());
    let mut drivers = BTreeMap::new();
    for ((package_id, configuration), groups) in &grouped {
        let first = groups.first().ok_or_else(|| internal("crate_empty"))?;
        let key = (package_id.clone(), configuration.clone());
        let job_id = assigned
            .get(&key)
            .ok_or_else(|| internal("crate_job_id_missing"))?
            .clone();
        let manifest = crate::internal_plan::manifest_for_key(&first.manifest_key);
        let display = crate_display_name(&first.package_name, &manifest, configuration);
        let use_mbx = groups.iter().any(|group| is_mbx(group));
        let use_nextest = groups.iter().any(|group| is_nextest(group));
        let driver = if use_mbx {
            RenderDriver::Mbx
        } else {
            RenderDriver::Cargo
        };
        let model = CrateJob {
            job_id: job_id.clone(),
            display_name: display,
            package_name: first.package_name.clone(),
            package_id: package_id.clone(),
            manifest,
            configuration: configuration.clone(),
            obligations: obligations_for(groups, catalog)?,
        };
        model.validate()?;
        let repo_has_mbx = crate::workflow::plan_uses_mbx(discovery);
        let mut job = render_job(
            label,
            &model,
            catalog,
            fetch_roots,
            use_mbx,
            use_nextest,
            repo_has_mbx,
            acquire,
        )?;
        // Allowlisted custom tasks run after the fixed obligations; an
        // empty allowlist (the default) appends nothing.
        job.steps
            .extend(crate::vectors::custom_task_steps(custom_tasks, catalog)?);
        drivers.insert(job_id.clone(), driver);
        jobs.push((job_id, job));
    }
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(CrateBuild { jobs, drivers })
}

/// True when the group becomes a crate-job obligation.
///
/// Test-less groups carry no command and package-less workspace groups
/// belong to the plan job, so neither is emitted as an obligation.
/// Shared with `plan` so its obligation list matches emission exactly.
pub(crate) fn is_runnable(group: &TaskGroup) -> bool {
    !group.no_test_targets && !group.package_id.is_empty()
}

/// Runnable groups by `(package_id, configuration)` in sorted order.
///
/// Skips groups without test targets (no command is emitted for them)
/// and package-less workspace groups (the plan job owns that scope).
fn group_runnable(groups: &[TaskGroup]) -> BTreeMap<(String, String), Vec<&TaskGroup>> {
    let mut grouped: BTreeMap<(String, String), Vec<&TaskGroup>> = BTreeMap::new();
    for group in groups {
        if !is_runnable(group) {
            continue;
        }
        grouped
            .entry((group.package_id.clone(), group.configuration.clone()))
            .or_default()
            .push(group);
    }
    grouped
}

/// ID-assignment inputs: one `(package_id, package_name, configuration)`
/// triple per crate group, named by its first member.
fn id_inputs(
    grouped: &BTreeMap<(String, String), Vec<&TaskGroup>>,
) -> BTreeSet<(String, String, String)> {
    grouped
        .iter()
        .filter_map(|((package_id, configuration), members)| {
            members.first().map(|first| {
                (
                    package_id.clone(),
                    first.package_name.clone(),
                    configuration.clone(),
                )
            })
        })
        .collect()
}

/// True when the group compiles through MBX.
fn is_mbx(group: &TaskGroup) -> bool {
    group.compile_driver == RustDriver::Mbx
}

/// True when the group runs tests through Nextest.
fn is_nextest(group: &TaskGroup) -> bool {
    group.test_runner == TestRunner::CargoNextest
}

/// In-crate obligation order: Format, Clippy, build, tests, doctests, docs.
fn kind_rank(kind: TaskKind) -> u8 {
    match kind {
        TaskKind::Fmt => 0,
        TaskKind::Clippy => 1,
        TaskKind::Build => 2,
        TaskKind::Test | TaskKind::Nextest => 3,
        TaskKind::Doctest => 4,
        TaskKind::Doc => 5,
    }
}

/// Ordered validated obligations for one crate's groups.
fn obligations_for(
    groups: &[&TaskGroup],
    catalog: &ToolCatalog,
) -> Result<Vec<CrateObligation>, OrchestratorError> {
    let executed: BTreeSet<&str> = groups.iter().map(|group| group.task_id.as_str()).collect();
    let mut ordered = groups.to_vec();
    ordered.sort_by(|left, right| {
        (kind_rank(left.kind), &left.task_id).cmp(&(kind_rank(right.kind), &right.task_id))
    });
    let mut obligations = Vec::with_capacity(ordered.len());
    for group in ordered {
        obligations.push(obligation_for(group, &executed, catalog)?);
    }
    Ok(obligations)
}

/// One obligation: identities, same-crate gates, fixed argv.
fn obligation_for(
    group: &TaskGroup,
    executed: &BTreeSet<&str>,
    catalog: &ToolCatalog,
) -> Result<CrateObligation, OrchestratorError> {
    let argv = crate::vectors::task_argv(group, catalog)?;
    let toolchain = crate::internal_plan::toolchain_id(group, catalog)?;
    let digest = crate::internal::plan_obligation::task_digest(&group.task_id, &argv, &toolchain)?;
    let matrix_id = matrix_id_for_task_group(STACK_ID, &group.task_id)?;
    let matrix_key = matrix_key_for_id(&matrix_id)?;
    Ok(CrateObligation {
        task_id: group.task_id.clone(),
        kind: group.kind.as_str().to_owned(),
        step_name: step_name_for(group.kind, &group.task_id),
        gated_by: gates_for(group, executed),
        matrix_key,
        task_digest: digest,
        run: argv,
    })
}

/// Sorted same-crate gates: quality gates plus data producers.
///
/// Gates naming skipped groups (test-less doctests) are vacuous: the
/// kind order still sequences the survivors, and dangling references
/// would fail the strictly-earlier validation.
fn gates_for(group: &TaskGroup, executed: &BTreeSet<&str>) -> Vec<String> {
    let mut gates: Vec<String> = group
        .gated_by
        .iter()
        .chain(group.depends_on.iter())
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
/// the plan), restore shared sources (or Cargo-only registry), then
/// MBX objects, then probe-and-fetch, then report-wrapped
/// obligations, then one always-on matrix-report upload per
/// obligation. Readers never save.
#[expect(
    clippy::too_many_arguments,
    reason = "one call site threads job scope plus driver selection"
)]
fn render_job(
    label: &str,
    model: &CrateJob,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    use_mbx: bool,
    use_nextest: bool,
    repo_has_mbx: bool,
    acquire: Option<&Step>,
) -> Result<Job, OrchestratorError> {
    let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
    steps.extend(acquire.cloned());
    steps.push(crate::matrix_step::download_plan_step()?);
    steps.push(crate::matrix_step::prepare_crate_tools_step(
        catalog,
        use_mbx,
        use_nextest,
    )?);
    steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    steps.extend(restore_step_for_crate(
        label,
        catalog,
        fetch_roots,
        use_mbx,
        repo_has_mbx,
    )?);
    steps.extend(mbx_objects_step(catalog, use_mbx)?);
    steps.extend(crate::source_prep::fetch_steps_for_crate(
        catalog,
        fetch_roots,
    )?);
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
        steps.push(crate::matrix_step::obligation_step(
            obligation,
            catalog,
            &downstream,
        )?);
    }
    for obligation in &model.obligations {
        steps.push(crate::matrix_step::matrix_upload_step(obligation)?);
    }
    Ok(Job {
        display_name: model.display_name.clone(),
        runs_on: label.to_owned(),
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps,
    })
}

/// Restore step for one crate: shared sources, or Cargo-only registry.
///
/// Lockless emits nothing. Cargo-only repos (no MBX anywhere) restore via
/// pinned `rust-cache` (read-only); every other lockful crate restores the
/// shared `actions/cache` snapshot (read-only, never saves the shared key).
fn restore_step_for_crate(
    label: &str,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    use_mbx: bool,
    repo_has_mbx: bool,
) -> Result<Option<Step>, OrchestratorError> {
    if fetch_roots.is_empty() {
        return Ok(None);
    }
    let target = velnor_actions_contract::target_for_runner_label(label).ok_or_else(|| {
        OrchestratorError::Contract {
            problem: format!("bad_label:{label}"),
        }
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

/// MBX objects restore for MBX crates only (WF-3.52).
///
/// The action installs the catalog MBX pin through its `version` input,
/// so action setup and the Mise-selected compiler share one proven
/// identity instead of a floating `latest` executable.
fn mbx_objects_step(
    catalog: &ToolCatalog,
    use_mbx: bool,
) -> Result<Option<Step>, OrchestratorError> {
    if !use_mbx {
        return Ok(None);
    }
    let uses = PinnedActionRef::new(
        "jdx/mr-boxington-action",
        None,
        MR_BOXINGTON_ACTION_SHA,
        MR_BOXINGTON_ACTION_VERSION,
    )?
    .uses_value();
    let mbx = catalog.version(PinnedTool::MrBoxington);
    Ok(mbx_step_for_driver(&uses, RenderDriver::Mbx, mbx)?)
}

#[cfg(test)]
#[path = "crate_jobs_tests.rs"]
mod crate_jobs_tests;

#[cfg(test)]
#[path = "crate_jobs_reports_tests.rs"]
mod crate_jobs_reports_tests;

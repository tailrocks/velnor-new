//! Build ordered crate jobs and route source-bound Nextest archives.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::config::{NextestTestSelection, NextestTestSelectionMode};
use velnor_actions_contract::{
    CrateJob, CrateObligation, Job, JobTimeout, ProposedTask, Stack, Step, WorkflowPolicy,
    matrix_id_for_task_group, matrix_key_for_id,
};
use velnor_actions_mise::{
    ArchiveIdentityInputs, NextestArchive, NextestDriver, PinnedTool, TaskCacheMode, ToolCatalog,
    archive_identity,
};
use velnor_actions_rust::CompileDriver;
use velnor_actions_workflow_renderer::steps::CompileDriver as RenderDriver;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::closure::resolve_closure_at_root;
use crate::internal_plan::identities::{extension_bundle_with_snapshot, platform_id_for_group};
use crate::internal_plan::snapshot::{ExecutionSnapshot, canonical_digest};
use crate::internal_plan::wire_w2::{ArchiveGate, check_archive_identity_with_source};
use crate::internal_plan::{nextest_config_for, toolchain_id};
use crate::matrix_step::step_name_for;
use crate::obligation_order::obligation_order_key;
use crate::workflow::wire_w1;
use crate::{matrix_step as matrix, source_prep, tofu_cache};

#[path = "crate_jobs_archive.rs"]
mod archive;
#[path = "crate_jobs_stage.rs"]
mod stage;

#[cfg(test)]
pub(crate) use stage::needs;
pub(crate) use stage::{is_mbx, is_nextest, is_opentofu, is_rust};

/// Ordered crate jobs and their driver selections.
pub(crate) struct CrateBuild {
    pub(crate) jobs: Vec<(String, Job)>,
    pub(crate) drivers: BTreeMap<String, RenderDriver>,
}

pub(crate) use archive::build_for_workflow;

/// Build ordered jobs for runnable crate obligations.
pub(crate) fn build_crate_jobs(
    label: &str,
    policy: WorkflowPolicy,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    acquire: Option<&Step>,
    max_parallel_jobs: u32,
) -> Result<CrateBuild, OrchestratorError> {
    archive::build_crate_jobs_inner(
        None,
        label,
        policy,
        discovery,
        &[],
        catalog,
        fetch_roots,
        acquire,
        max_parallel_jobs,
    )
}

fn source_bound_archive(
    root: &Path,
    label: &str,
    discovery: &Discovery,
    snapshot: &ExecutionSnapshot,
    task: &ProposedTask,
    source_task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<Option<(String, NextestArchive, NextestDriver)>, OrchestratorError> {
    let config = nextest_config_for(discovery, source_task);
    let bundle = extension_bundle_with_snapshot(
        snapshot,
        discovery,
        source_task,
        Some(root),
        config.as_deref(),
    );
    let toolchain = toolchain_id(source_task, catalog).map_err(internal_contract)?;
    let platform = platform_id_for_group(label, source_task).map_err(internal_contract)?;
    let mut reads = velnor_actions_tofu::FileCache::new();
    let closure = resolve_closure_at_root(
        root,
        source_task,
        config.as_deref(),
        bundle.graph_digest(),
        &toolchain,
        &platform,
        &mut reads,
    )
    .map_err(internal_contract)?;
    if closure.verify_complete().is_err() {
        return Ok(None);
    }
    let source = canonical_digest(&closure).map_err(internal_contract)?;
    if check_archive_identity_with_source(
        task,
        &toolchain,
        &platform,
        bundle.config_digest(),
        Some(&source),
    )? == ArchiveGate::SourceUnbound
    {
        return Ok(None);
    }
    let driver = match CompileDriver::parse(&source_task.identity.compile_driver)
        .map_err(crate::internal::internal_contract)?
    {
        CompileDriver::Cargo => NextestDriver::Cargo,
        CompileDriver::Mbx => NextestDriver::Mbx,
    };
    let target =
        (source_task.identity.target != "host").then_some(source_task.identity.target.as_str());
    let archive = NextestArchive::with_profile(
        driver,
        &source_task.display_name,
        &source_task.identity.features,
        target,
        &source_task.runner_profile,
    )
    .and_then(|archive| archive.with_manifest_path(&source_task.identity.unit_path))
    .map_err(|err| internal(&format!("archive_inputs_rejected:{err}")))?;
    let identity = archive_identity(
        &archive,
        &ArchiveIdentityInputs {
            source_digest: &source,
            profile: &source_task.configuration,
            toolchain_id: &toolchain,
            runtime: &source_task.identity.compile_driver,
            test_runner: catalog.version(PinnedTool::Nextest),
            format: "tar.zst",
            platform_id: &platform,
            config_digest: bundle.config_digest(),
        },
    )
    .map_err(|err| internal(&format!("archive_identity_rejected:{err}")))?;
    Ok(Some((identity, archive, driver)))
}

fn selection_for(
    task: &ProposedTask,
    selectors: &[NextestTestSelection],
) -> Result<(Option<String>, Option<String>), OrchestratorError> {
    let selected = selectors
        .iter()
        .find(|item| item.package == task.display_name);
    let filter = payload_option(&task.payload, "--filterset")?;
    let run_ignored = payload_option(&task.payload, "--run-ignored")?;
    if let Some(selection) = selected {
        let expected = selection_filter(selection);
        if filter.as_deref() != Some(expected.as_str())
            || run_ignored.as_deref() != Some("only")
            || selection.mode != NextestTestSelectionMode::IgnoredOnly
        {
            return Err(internal("nextest_selector_payload_mismatch"));
        }
        return Ok((Some(expected), Some("only".to_owned())));
    }
    if filter.is_some() {
        return Err(internal("nextest_filter_without_selector"));
    }
    if run_ignored
        .as_deref()
        .is_some_and(|mode| !["all", "only", "ignored-only"].contains(&mode))
    {
        return Err(internal("nextest_run_ignored_invalid"));
    }
    Ok((None, run_ignored))
}

fn payload_option(payload: &[OsString], flag: &str) -> Result<Option<String>, OrchestratorError> {
    let mut matches = payload
        .iter()
        .enumerate()
        .filter(|(_, item)| item.to_str() == Some(flag));
    let Some((index, _)) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(internal("nextest_payload_option_duplicated"));
    }
    payload
        .get(index + 1)
        .and_then(|value| value.to_str())
        .map(|value| Some(value.to_owned()))
        .ok_or_else(|| internal("nextest_payload_option_missing_value"))
}

fn selection_filter(selection: &NextestTestSelection) -> String {
    let tests = selection
        .test_ids
        .iter()
        .map(|id| format!("test(={id})"))
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "package(={}) & binary(={}) & ({tests})",
        selection.package, selection.binary
    )
}

/// Exclude commandless, package-less, and Mise tasks from crate jobs.
pub(crate) fn is_runnable(task: &ProposedTask) -> bool {
    !task.no_targets
        && !task.identity.unit_id.is_empty()
        && Stack::from_id(&task.stack_id) != Some(Stack::Mise)
}

#[cfg(test)]
fn obligation_rank(task: &ProposedTask) -> u32 {
    obligation_order_key(&task.stack_id, &task.task_kind, &task.task_id).0
}

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

/// Keep same-crate gates whose prerequisite tasks are emitted.
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

/// Render setup, routed obligations, and report upload for one crate.
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
    acquire: Option<&Step>,
    max_parallel_jobs: u32,
    archives: &archive::ArchivePlan,
) -> Result<Job, OrchestratorError> {
    let mut steps = vec![wire_w1::checkout_step()?];
    steps.extend(acquire.cloned());
    steps.push(matrix::download_plan_step()?);
    let suite = matrix::suite_for_package(&model.package_name);
    let needs_validators = matrix::crate_needs_generate_validators(policy, suite);
    let install_opentofu = matrix::prepare_install_opentofu(policy, suite, use_opentofu);
    steps.push(matrix::prepare_crate_tools_step(
        catalog,
        use_rust,
        use_nextest,
        install_opentofu,
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
    )?);
    if use_opentofu {
        let root = tofu_cache::tofu_root_for_obligations(&model.obligations)?;
        steps.extend(tofu_cache::provider_cache_step_for_tofu_root(
            label, catalog, &root,
        )?);
    }
    if use_mbx {
        steps.extend(crate::mbx_preflight::steps_for_catalog(catalog)?);
    }
    if use_rust {
        steps.extend(source_prep::fetch_steps_for_crate(catalog, fetch_roots)?);
    }
    steps.extend(wire_w1::maybe_task_cache_steps(
        None,
        TaskCacheMode::Off,
        "",
    )?);
    steps.extend(archives.consumer_steps(model, catalog, max_parallel_jobs, use_opentofu)?);
    steps.push(matrix::crate_upload_step(&model.job_id)?);
    Ok(Job {
        display_name: model.display_name.clone(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: archives.dependencies(&model.obligations),
        condition: None,
        permissions: None,
        environment: None,
        steps,
    })
}

/// Restore the shared Cargo source subset; tofu uses its provider cache.
fn restore_step_for_crate(
    label: &str,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    use_rust: bool,
) -> Result<Option<Step>, OrchestratorError> {
    if !use_rust || fetch_roots.is_empty() {
        return Ok(None);
    }
    let target = velnor_actions_contract::ReleaseTarget::for_runner_label(label)
        .map(velnor_actions_contract::ReleaseTarget::triple)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("bad_label:{label}"),
        })?;
    let rust = catalog.version(PinnedTool::Rust);
    let key = crate::source_cache::sources_cache_key(target, rust, fetch_roots)?;
    let prefix = crate::source_cache::sources_restore_prefix(&key);
    crate::source_cache::sources_restore_step(&key, &[prefix]).map(Some)
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

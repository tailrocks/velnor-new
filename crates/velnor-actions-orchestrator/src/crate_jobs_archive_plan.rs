//! Derive source-bound Nextest archive producers and shard routes.

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::config::NextestTestSelection;
use velnor_actions_contract::{
    CrateJob, Job, JobTimeout, ProposedTask, Step, VelnorConfig, WorkflowPolicy,
    crate_display_name, split_shard_suffix,
};
use velnor_actions_mise::{
    NextestArchive, NextestDriver, NextestPartition, ToolCatalog, archive_write_required,
    requires_archive_transfer,
};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::steps::{CompileDriver as RenderDriver, RUN_KEY_EXPR};

use crate::OrchestratorError;
use crate::crate_job_ids::{assign_group_ids, group_is_tofu, group_runnable};
use crate::discover::Discovery;
use crate::internal::internal;
use crate::internal_plan::snapshot::ExecutionSnapshot;

use super::{ARCHIVE_PATH, ArchivePlan, ArchiveRoute, Producer};

pub(crate) fn build_for_workflow(
    root: &Path,
    config: &VelnorConfig,
    label: &str,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    acquire: Option<&Step>,
) -> Result<super::super::CrateBuild, OrchestratorError> {
    let selectors = config
        .stacks
        .rust
        .as_ref()
        .map_or(&[][..], |rust| rust.nextest_test_selectors.as_slice());
    build_crate_jobs_inner(
        Some(root),
        label,
        config.workflow.policy,
        discovery,
        selectors,
        catalog,
        fetch_roots,
        acquire,
        config.workflow.max_parallel_jobs,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "workflow inputs stay independently explicit"
)]
pub(in crate::crate_jobs) fn build_crate_jobs_inner(
    root: Option<&Path>,
    label: &str,
    policy: WorkflowPolicy,
    discovery: &Discovery,
    selectors: &[NextestTestSelection],
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    acquire: Option<&Step>,
    max_parallel_jobs: u32,
) -> Result<super::super::CrateBuild, OrchestratorError> {
    let mut archives = match root {
        Some(root) => build_archive_plan(root, label, discovery, selectors, catalog, fetch_roots)?,
        None => ArchivePlan::empty(),
    };
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
            velnor_actions_contract::tofu_display_name(&first.display_name)
        } else {
            crate_display_name(&first.display_name, &manifest, configuration)
        };
        let use_rust = tasks.iter().any(|task| super::super::is_rust(task));
        let use_mbx = tasks.iter().any(|task| super::super::is_mbx(task));
        let use_nextest = tasks.iter().any(|task| super::super::is_nextest(task));
        let use_opentofu = tasks.iter().any(|task| super::super::is_opentofu(task));
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
            obligations: super::super::obligations_for(tasks, catalog)?,
        };
        model.validate()?;
        let job = super::super::render_job(
            label,
            policy,
            &model,
            catalog,
            fetch_roots,
            use_rust,
            use_mbx,
            use_nextest,
            use_opentofu,
            acquire,
            max_parallel_jobs,
            &archives,
        )?;
        drivers.insert(job_id.clone(), driver);
        if use_opentofu {
            tofu_ids.push(job_id.clone());
        }
        jobs.push((job_id, job));
    }
    super::super::stage::stage_tofu_root_jobs(&mut jobs, &tofu_ids, max_parallel_jobs);
    for (job_id, job, driver) in archives.take_jobs() {
        drivers.insert(job_id.clone(), driver);
        jobs.push((job_id, job));
    }
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(super::super::CrateBuild { jobs, drivers })
}

pub(in crate::crate_jobs) fn build_archive_plan(
    root: &Path,
    label: &str,
    discovery: &Discovery,
    selectors: &[NextestTestSelection],
    catalog: &ToolCatalog,
    fetch_roots: &[String],
) -> Result<ArchivePlan, OrchestratorError> {
    let mut plan = ArchivePlan::empty();
    validate_selectors(selectors)?;
    let snapshot = ExecutionSnapshot::build(discovery);
    for task in &discovery.proposals {
        if task.task_kind != "nextest" || !super::super::is_runnable(task) {
            continue;
        }
        let Some((base_id, index, count)) = split_shard_suffix(&task.task_id) else {
            if task.task_id.contains("/shard-") {
                return Err(internal("malformed_nextest_shard_id"));
            }
            continue;
        };
        if !archive_write_required(count) || !requires_archive_transfer(count) {
            continue;
        }
        let partition = NextestPartition::new(index, count)
            .map_err(|err| internal(&format!("nextest_partition_rejected:{err}")))?;
        let (filter, run_ignored) = super::super::selection_for(task, selectors)?;
        let mut base_task = task.clone();
        base_task.task_id = base_id.to_owned();
        let Some((identity, archive, driver)) = super::super::source_bound_archive(
            root, label, discovery, &snapshot, task, &base_task, catalog,
        )?
        else {
            continue;
        };
        let hex = identity
            .strip_prefix("b3-")
            .ok_or_else(|| internal("archive_digest_prefix"))?;
        let producer_id = format!("nextest-archive-{hex}");
        let artifact_name = format!("velnor-nextest-{RUN_KEY_EXPR}-{hex}");
        if !plan.producers.contains_key(&producer_id) {
            let job = archive_job(
                label,
                task,
                &archive,
                driver,
                catalog,
                fetch_roots,
                &artifact_name,
            )?;
            plan.producers.insert(
                producer_id.clone(),
                Producer {
                    job,
                    driver: if driver == NextestDriver::Mbx {
                        RenderDriver::Mbx
                    } else {
                        RenderDriver::Cargo
                    },
                },
            );
        }
        plan.routes.insert(
            task.task_id.clone(),
            ArchiveRoute {
                producer_id,
                artifact_name,
                driver,
                partition,
                profile: task.runner_profile.clone(),
                run_ignored,
                filter,
            },
        );
    }
    Ok(plan)
}

fn validate_selectors(selectors: &[NextestTestSelection]) -> Result<(), OrchestratorError> {
    for (index, selector) in selectors.iter().enumerate() {
        selector
            .validate(
                ".velnor/config.toml",
                &format!("stacks.rust.nextest_test_selectors[{index}]"),
            )
            .map_err(crate::internal::internal_contract)?;
    }
    Ok(())
}

fn archive_job(
    label: &str,
    task: &ProposedTask,
    archive: &NextestArchive,
    driver: NextestDriver,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    artifact_name: &str,
) -> Result<Job, OrchestratorError> {
    let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
    steps.push(crate::matrix_step::prepare_crate_tools_step(
        catalog, true, true, false, false,
    )?);
    steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    steps.extend(super::super::restore_step_for_crate(
        label,
        catalog,
        fetch_roots,
        true,
    )?);
    if driver == NextestDriver::Mbx {
        steps.extend(crate::mbx_preflight::steps_for_catalog(catalog)?);
    }
    steps.extend(crate::source_prep::fetch_steps_for_crate(
        catalog,
        fetch_roots,
    )?);
    let argv = archive
        .argv(catalog)
        .into_iter()
        .map(|arg| {
            arg.into_string()
                .map_err(|_| internal("archive_argv_not_utf8"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    crate::matrix_step::nextest_archive_steps(
        &mut steps,
        catalog,
        argv,
        artifact_name,
        ARCHIVE_PATH,
    )?;
    Ok(Job {
        display_name: crate_display_name(
            &format!("Nextest archive {}", task.display_name),
            &task.identity.unit_path,
            &task.configuration,
        ),
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

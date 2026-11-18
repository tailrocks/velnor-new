// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::ledger_cli::{LEDGER_CLI_JOB_ID, LedgerCliSource};
use velnor_actions_contract::{
    CacheAccess, CrateJob, Job, JobTimeout, ProposedTask, Stack, Step,
    crate_display_name, tofu_display_name,
};
use velnor_actions_mise::TaskCacheMode;
use velnor_actions_workflow_renderer::steps::CompileDriver as RenderDriver;

use crate::OrchestratorError;
use crate::crate_job_ids::{CrateGroupKey, JobOwners, group_is_tofu, group_runnable};
use crate::config::private_wiring::PrivateWiring;
use crate::internal::internal;

#[path = "crate_jobs_inputs.rs"]
mod inputs;
pub(crate) use inputs::CrateJobInputs;

#[path = "crate_jobs_stage.rs"]
mod stage;

#[path = "crate_jobs_gates.rs"]
mod gates;

#[path = "crate_jobs_build_jobs.rs"]
mod build_jobs;
use build_jobs::{group_plan_for, reject_seed_needs};

#[path = "crate_jobs_ledger_cli.rs"]
mod ledger_cli;

#[path = "crate_jobs_steps.rs"]
mod job_steps;
#[path = "crate_jobs_obligations.rs"]
mod obligations;
use job_steps::obligation_steps;
use obligations::obligations_for;

pub(crate) use stage::{is_nextest, is_opentofu, is_rust};

pub(crate) struct CrateBuild {
    pub(crate) ledger_cli: Option<(String, Job)>,
    pub(crate) jobs: Vec<(String, Job)>,
    pub(crate) drivers: BTreeMap<String, RenderDriver>,
}

pub(crate) fn build_crate_jobs(
    input: CrateJobInputs<'_>,
    private: Option<&PrivateWiring>,
) -> Result<CrateBuild, OrchestratorError> {
    let CrateJobInputs { discovery, .. } = input;
    let grouped = group_runnable(&discovery.proposals)?;
    let owners = JobOwners::from_grouped(&discovery.proposals, &grouped)?;
    let mut jobs = Vec::with_capacity(grouped.len());
    let mut drivers = BTreeMap::new();
    for (key, tasks) in &grouped {
        let (job_id, job) = build_group_job(&input, &owners, key, tasks)?;
        drivers.insert(job_id.clone(), RenderDriver::Cargo);
        jobs.push((job_id, job));
    }
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    let ledger_cli = (LedgerCliSource::for_policy(input.policy) == LedgerCliSource::SameRunBuild
        && !jobs.is_empty())
    .then(|| {
        let lane = crate::matrix_step::LaneTools::union(input.policy, input.catalog, &grouped);
        ledger_cli::ledger_cli_job(&input, &lane, private)
    })
    .transpose()?
    .map(|job| (LEDGER_CLI_JOB_ID.to_owned(), job));
    if let Some((id, _)) = &ledger_cli {
        drivers.insert(id.clone(), RenderDriver::Cargo);
    }
    Ok(CrateBuild {
        ledger_cli,
        jobs,
        drivers,
    })
}

fn build_group_job(
    input: &CrateJobInputs<'_>,
    owners: &JobOwners<'_>,
    key: &CrateGroupKey,
    tasks: &[&ProposedTask],
) -> Result<(String, Job), OrchestratorError> {
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
    let model = CrateJob {
        job_id: job_id.clone(),
        display_name: display,
        package_name: first.display_name.clone(),
        package_id: key.package_id.clone(),
        manifest,
        configuration: key.configuration.clone(),
        obligations: obligations_for(tasks, input.catalog)?,
    };
    model.validate()?;
    let needs = gates::external_needs(
        tasks,
        owners,
        &job_id,
        LedgerCliSource::for_policy(input.policy),
    )?;
    reject_seed_needs(&needs, &job_id)?;
    let workspace_root =
        crate::source_prep::workspace_root_for_manifest(&model.manifest, input.fetch_roots);
    let plan = group_plan_for(&model, workspace_root, input.label, input.timeout)?;
    let tools = crate::matrix_step::LaneTools::of_group(input.policy, input.catalog, tasks);
    let job = render_job(input, &model, &tools, needs, plan.timeout_minutes)?;
    Ok((job_id, job))
}

pub(crate) fn is_runnable(task: &ProposedTask) -> bool {
    !task.no_targets
        && !task.identity.unit_id.is_empty()
        && !matches!(
            Stack::from_id(&task.stack_id),
            Some(Stack::Mise | Stack::Native)
        )
}


fn stack_cache_steps(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
) -> Result<Vec<Step>, OrchestratorError> {
    let root = crate::tofu_cache::tofu_root_for_obligations(&model.obligations)?;
    let roots = crate::tofu_cache::tofu_cache_roots(input.discovery)?;
    Ok(crate::tofu_cache::provider_cache_step_for_tofu_root(&root, &roots)?.into_iter().collect())
}

fn render_job(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    tools: &crate::matrix_step::LaneTools,
    needs: Vec<String>,
    timeout: JobTimeout,
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
    steps.push(crate::matrix_step::prepare_crate_tools_step(
        catalog, tools,
    )?);
    steps.extend(crate::matrix_step::verify_validators_step(catalog, tools));
    steps.extend(crate::matrix_step::prepare_runtime_gh_step(
        policy, suite, catalog,
    )?);
    steps.extend(crate::matrix_step::prepare_runtime_cosign_step(
        policy, suite, catalog,
    )?);
    steps.extend(crate::source_cache::restore_step_for_crate(
        fetch_roots,
        false,
    )?);
    steps.extend(stack_cache_steps(input, model)?);
    steps.extend(crate::workflow::wire_w1::maybe_task_cache_steps(
        None,
        TaskCacheMode::Off,
        "",
    )?);
    steps.extend(obligation_steps(input, model)?);
    steps.extend(velnor_actions_workflow_renderer::crate_group_ledger_steps(
        policy,
    )?);
    steps.push(crate::matrix_step::crate_upload_step(&model.job_id)?);
    Ok(Job {
        services: Vec::new(),
        outputs: BTreeMap::new(),
        display_name: model.display_name.clone(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: timeout,
        needs,
        condition: None,
        permissions: None,
        environment: None,
        env: velnor_actions_contract::JobEnv::Unset,
        steps,
        cache_access: CacheAccess::Read,
    })
}

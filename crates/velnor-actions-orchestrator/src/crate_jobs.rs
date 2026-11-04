// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CacheAccess, CompiledStateMode, CrateJob, Job, JobTimeout, ProposedTask, Stack, Step,
    VelnorConfig, WorkflowPolicy, crate_display_name, tofu_display_name,
};
use velnor_actions_contract::workflow::cache_family::S256;
use velnor_actions_contract::workflow::ledger_cli::{LEDGER_CLI_JOB_ID, LedgerCliSource};
use velnor_actions_mise::{TaskCacheMode, ToolCatalog};
use velnor_actions_workflow_renderer::steps::CompileDriver as RenderDriver;

use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;
use crate::crate_job_ids::{CrateGroupKey, JobOwners, group_is_tofu, group_runnable};
use crate::discover::Discovery;
use crate::internal::internal;

#[path = "crate_jobs_inputs.rs"]
mod inputs;
pub(crate) use inputs::CrateJobInputs;

#[path = "state_producer.rs"]
pub(crate) mod state_producer;

#[path = "crate_jobs_lanes.rs"]
mod lanes;
use lanes::{ProducerLanes, lane_records};

#[path = "crate_jobs_stage.rs"]
mod stage;

#[path = "crate_jobs_gates.rs"]
mod gates;

#[path = "crate_jobs_build_jobs.rs"]
mod build_jobs;
use build_jobs::{group_plan_for, reject_seed_needs, reject_unknown_crates};

#[path = "crate_jobs_ledger_cli.rs"]
mod ledger_cli;

#[path = "crate_jobs_steps.rs"]
mod job_steps;
#[path = "crate_jobs_obligations.rs"]
mod obligations;
use job_steps::obligation_steps;
use obligations::obligations_for;

pub(crate) use stage::{is_mbx, is_nextest, is_opentofu, is_rust};

pub(crate) struct CrateBuild {
    pub(crate) ledger_cli: Option<(String, Job)>,
    pub(crate) producers: Vec<(String, Job)>,
    pub(crate) state_recipes: BTreeMap<String, S256>,
    pub(crate) jobs: Vec<(String, Job)>,
    pub(crate) drivers: BTreeMap<String, RenderDriver>,
}

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
    build_crate_jobs_with_seed_timeout(
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
            compiled_state: config.stacks.compiled_state(),
            seed_parallel_targets: config
                .stacks
                .rust
                .as_ref()
                .is_some_and(|rust| rust.seed_parallel_targets),
        },
        private,
        JobTimeout::new(config.workflow.state_producer_timeout_minutes)?,
    )
}

fn build_crate_jobs_with_seed_timeout(
    input: CrateJobInputs<'_>,
    private: Option<&PrivateWiring>,
    seed_timeout: JobTimeout,
) -> Result<CrateBuild, OrchestratorError> {
    let CrateJobInputs { discovery, .. } = input;
    let grouped = group_runnable(&discovery.proposals)?;
    let owners = JobOwners::from_grouped(&discovery.proposals, &grouped)?;
    let mut jobs = Vec::with_capacity(grouped.len());
    let mut drivers = BTreeMap::new();
    let lane = crate::matrix_step::LaneTools::union(input.policy, input.catalog, &grouped);
    let mut producer_roots: BTreeMap<String, state_producer::ProducerRoot> = BTreeMap::new();
    for (key, tasks) in &grouped {
        let output = build_group_job(&input, &lane, &owners, key, tasks, private)?;
        if let Some(lanes) = output.producer {
            let entry = producer_roots
                .entry(lanes.root.clone())
                .or_insert_with(|| state_producer::ProducerRoot {
                    root: lanes.root.clone(),
                    lanes: Vec::new(),
                });
            entry.lanes.extend(lanes.lanes);
        }
        drivers.insert(output.job_id.clone(), output.driver);
        jobs.push((output.job_id, output.job));
    }
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    let ledger_cli = (LedgerCliSource::for_policy(input.policy) == LedgerCliSource::SameRunBuild
        && !jobs.is_empty())
    .then(|| ledger_cli::ledger_cli_job(&input, &lane, private))
    .transpose()?
    .map(|job| (LEDGER_CLI_JOB_ID.to_owned(), job));
    if let Some((id, _)) = &ledger_cli {
        drivers.insert(id.clone(), RenderDriver::Cargo);
    }
    let roots: Vec<_> = producer_roots.into_values().collect();
    let recipe_jobs =
        state_producer::state_producer_jobs(&input, &lane, &roots, private, seed_timeout)?;
    let state_recipes = state_producer::recipe_digests(&recipe_jobs)?;
    let producers = match input.compiled_state {
        CompiledStateMode::Union => recipe_jobs,
        CompiledStateMode::Off => Vec::new(),
    };
    for (id, _) in &producers {
        drivers.insert(id.clone(), RenderDriver::Mbx);
    }
    Ok(CrateBuild {
        ledger_cli,
        producers,
        state_recipes,
        jobs,
        drivers,
    })
}

struct GroupOutput {
    job_id: String,
    job: Job,
    driver: RenderDriver,
    producer: Option<ProducerLanes>,
}


fn build_group_job(
    input: &CrateJobInputs<'_>,
    lane: &crate::matrix_step::LaneTools,
    owners: &JobOwners<'_>,
    key: &CrateGroupKey,
    tasks: &[&ProposedTask],
    private: Option<&PrivateWiring>,
) -> Result<GroupOutput, OrchestratorError> {
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
    let use_opentofu = tasks.iter().any(|task| is_opentofu(task));
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
    let tools = if use_rust {
        lane.clone()
    } else {
        crate::matrix_step::LaneTools::of_group(input.policy, input.catalog, tasks)
    };
    let job = render_job(
        input,
        &model,
        use_rust,
        use_mbx,
        use_opentofu,
        &tools,
        needs,
        plan.timeout_minutes,
        private,
    )?;
    let driver = if use_mbx {
        RenderDriver::Mbx
    } else {
        RenderDriver::Cargo
    };
    let producer = use_mbx.then(|| ProducerLanes {
        root: workspace_root.to_owned(),
        lanes: lane_records(&job, &model),
    });
    Ok(GroupOutput {
        job_id,
        job,
        driver,
        producer,
    })
}

pub(crate) fn is_runnable(task: &ProposedTask) -> bool {
    !task.no_targets
        && !task.identity.unit_id.is_empty()
        && !matches!(
            Stack::from_id(&task.stack_id),
            Some(Stack::Mise | Stack::Native)
        )
}

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

fn stack_cache_steps(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    use_mbx: bool,
    use_opentofu: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::new();
    if use_opentofu {
        let root = crate::tofu_cache::tofu_root_for_obligations(&model.obligations)?;
        let roots = crate::tofu_cache::tofu_cache_roots(input.discovery)?;
        steps.extend(crate::tofu_cache::provider_cache_step_for_tofu_root(
            &root, &roots,
        )?);
    }
    if use_mbx {
        let root =
            crate::source_prep::workspace_root_for_manifest(&model.manifest, input.fetch_roots);
        let working_dir = (!root.is_empty()).then_some(root);
        steps.extend(crate::mbx_preflight::steps_for_catalog(
            input.catalog,
            velnor_actions_workflow_renderer::steps::MbxTransport::SeedRead,
            working_dir,
        )?);
    }
    Ok(steps)
}

#[expect(
    clippy::too_many_arguments,
    reason = "one call site threads job scope plus driver selection"
)]
fn render_job(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    use_rust: bool,
    use_mbx: bool,
    use_opentofu: bool,
    tools: &crate::matrix_step::LaneTools,
    needs: Vec<String>,
    timeout: JobTimeout,
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
    if use_rust {
        steps.push(crate::workflow::verify_restored_rust_step(catalog));
    }
    steps.push(crate::matrix_step::prepare_crate_tools_step(catalog, tools)?);
    steps.extend(crate::matrix_step::verify_validators_step(catalog, tools));
    if use_rust {
        steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
        steps.push(crate::workflow::verify_rust_manifest_step(catalog));
    }
    steps.extend(crate::source_cache::restore_step_for_crate(
        fetch_roots,
        use_rust,
    )?);
    steps.extend(stack_cache_steps(input, model, use_mbx, use_opentofu)?);
    if use_rust {
        steps.extend(rust_source_steps(input, model, needs_validators, private)?);
    }
    steps.extend(crate::workflow::wire_w1::maybe_task_cache_steps(
        None,
        TaskCacheMode::Off,
        "",
    )?);
    steps.extend(obligation_steps(input, model, use_opentofu)?);
    if use_rust {
        let root = crate::source_prep::workspace_root_for_manifest(&model.manifest, fetch_roots);
        steps.push(velnor_actions_workflow_renderer::mbx_stats::mbx_stats_step(
            &model.job_id,
            &crate::source_prep::workspace_crate_names(&input.discovery.workspaces, root),
        )?);
    }
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
        condition: (use_rust && !use_opentofu)
            .then(|| velnor_actions_workflow_renderer::not_affected_gate(&model.job_id)),
        permissions: None,
        environment: None,
        env: velnor_actions_contract::JobEnv::Unset,
        steps,
        cache_access: CacheAccess::Read,
    })
}

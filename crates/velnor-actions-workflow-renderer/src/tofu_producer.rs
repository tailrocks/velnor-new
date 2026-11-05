// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::TOFU_PROVIDERS_PROBE_MISS_CONDITION;
use velnor_actions_contract::workflow::crate_job::TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX;
use velnor_actions_contract::workflow::{
    CacheAccess, CacheWriter, validate_tofu_providers_producer,
};
use velnor_actions_contract::{Job, Step, StepKind, StepRole};
use velnor_actions_tofu::TofuTaskKind;

use crate::RenderError;
use crate::tofu_cache::{
    TOFU_PROVIDER_ADMISSION_USES, tofu_providers_prepare_step, tofu_providers_probe_step,
    tofu_providers_save_step,
};

const PRODUCER_DISPLAY_PREFIX: &str = "Tofu providers producer / ";
const FETCH_STEP_NAME: &str = "Fetch Tofu providers";
const TASK_ID_ENV: &str = "VELNOR_TASK_ID";
const OPENTOFU_SPEC_PREFIX: &str = "opentofu@";
const LOCATOR_PREFIX: &str = "b3-";
const SHORT_LOCATOR_LEN: usize = 8;

struct ProviderProducerPlan<'a> {
    id: String,
    key: String,
    path: String,
    donor: &'a Job,
}

pub(crate) fn add_tofu_providers_producers(
    jobs: &mut BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    let mut built = Vec::new();
    for plan in plan(jobs)? {
        let job = build(&plan)?;
        validate_tofu_providers_producer(&job, &plan.id).map_err(RenderError::Contract)?;
        built.push((plan.id, job));
    }
    for (id, job) in built {
        if jobs.insert(id.clone(), job).is_some() {
            return Err(invalid(&format!("id_collision:{id}")));
        }
    }
    Ok(())
}

fn plan(jobs: &BTreeMap<String, Job>) -> Result<Vec<ProviderProducerPlan<'_>>, RenderError> {
    let mut donors: BTreeMap<(String, String), (&String, &Job)> = BTreeMap::new();
    for (id, job) in jobs {
        if job.producer_kind().is_some() {
            continue;
        }
        let Some(binding) = restore_binding(job) else {
            continue;
        };
        if init_step(job).is_none() {
            continue;
        }
        donors.entry(binding).or_insert((id, job));
    }
    let mut plans: Vec<ProviderProducerPlan<'_>> = Vec::with_capacity(donors.len());
    for ((key, path), (donor_id, donor)) in donors {
        let short = path
            .rsplit('/')
            .next()
            .and_then(|slug| slug.strip_prefix(LOCATOR_PREFIX))
            .and_then(|digest| digest.get(..SHORT_LOCATOR_LEN))
            .ok_or_else(|| invalid(&format!("path_shape:{donor_id}")))?;
        let id = format!("{TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX}{short}");
        if plans.iter().any(|existing| existing.id == id) {
            return Err(invalid(&format!("id_collision:{id}")));
        }
        plans.push(ProviderProducerPlan {
            id,
            key,
            path,
            donor,
        });
    }
    Ok(plans)
}

fn restore_binding(job: &Job) -> Option<(String, String)> {
    job.steps.iter().find_map(|step| {
        let StepKind::Action { uses, with, .. } = &step.kind else {
            return None;
        };
        (step.role == Some(StepRole::TofuProvidersRestore) && uses == TOFU_PROVIDER_ADMISSION_USES)
            .then(|| {
                Some((
                    with.get("cache-key")?.clone(),
                    with.get("cache-path")?.clone(),
                ))
            })
            .flatten()
    })
}

fn init_step(job: &Job) -> Option<&Step> {
    job.steps.iter().find(|step| {
        step.role == Some(StepRole::TofuProviderUse)
            && task_id(step).is_some_and(|task| task.split('/').nth(3) == Some("init"))
    })
}

fn task_id(step: &Step) -> Option<&str> {
    match &step.kind {
        StepKind::Shell { env, .. } => env.get(TASK_ID_ENV).map(String::as_str),
        _ => None,
    }
}

fn build(plan: &ProviderProducerPlan<'_>) -> Result<Job, RenderError> {
    let donor = plan.donor;
    let mut steps = Vec::new();
    for role in [StepRole::Checkout, StepRole::MiseSetup] {
        steps.push(donor_step(donor, role, &plan.id)?.clone());
    }
    steps.push(tofu_providers_probe_step(&plan.key, &plan.path)?);
    steps.push(gated(install_step(donor, &plan.id)?));
    steps.push(tofu_providers_prepare_step(&plan.path)?);
    steps.push(fetch_step(donor, &plan.id)?);
    steps.push(tofu_providers_save_step(&plan.key, &plan.path)?);
    Ok(Job {
        matrix: None,
        display_name: format!(
            "{PRODUCER_DISPLAY_PREFIX}{}",
            plan.id
                .strip_prefix(TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX)
                .unwrap_or_default()
        ),
        runs_on: donor.runs_on.clone(),
        check_runner: None,
        timeout_minutes: crate::cache_save_budget::lift_to_save_budget(donor.timeout_minutes)?,
        needs: vec![crate::render::PLAN_JOB_ID.to_owned()],
        condition: Some(CacheWriter::TofuProvidersProducer.condition()),
        permissions: None,
        cache_access: CacheAccess::Writer(CacheWriter::TofuProvidersProducer),
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: velnor_actions_contract::JobEnv::Unset,
        steps,
    })
}

fn donor_step<'a>(donor: &'a Job, role: StepRole, id: &str) -> Result<&'a Step, RenderError> {
    donor
        .steps
        .iter()
        .find(|step| step.role == Some(role))
        .ok_or_else(|| invalid(&format!("donor_step_missing:{id}:{role:?}")))
}

fn gated(mut step: Step) -> Step {
    step.condition = Some(TOFU_PROVIDERS_PROBE_MISS_CONDITION.to_owned());
    step
}

fn opentofu_spec(donor: &Job, id: &str) -> Result<String, RenderError> {
    let StepKind::Shell { run, .. } = &donor_step(donor, StepRole::PreparePinnedTools, id)?.kind
    else {
        return Err(invalid(&format!("donor_install_shape:{id}")));
    };
    let mut specs = run
        .iter()
        .filter(|arg| arg.starts_with(OPENTOFU_SPEC_PREFIX));
    match (specs.next(), specs.next()) {
        (Some(spec), None) => Ok(spec.clone()),
        _ => Err(invalid(&format!("donor_opentofu_spec:{id}"))),
    }
}

fn install_step(donor: &Job, id: &str) -> Result<Step, RenderError> {
    crate::tools_producer::specs_step(&[opentofu_spec(donor, id)?], id)
}

fn fetch_step(donor: &Job, id: &str) -> Result<Step, RenderError> {
    let init = init_step(donor).ok_or_else(|| invalid(&format!("donor_init_missing:{id}")))?;
    let StepKind::Shell { env, .. } = &init.kind else {
        return Err(invalid(&format!("donor_init_shape:{id}")));
    };
    let key = task_id(init)
        .and_then(|task| task.split('/').nth(2))
        .ok_or_else(|| invalid(&format!("donor_init_task_id:{id}")))?;
    let root = velnor_actions_tofu::root_for_key(key)
        .map_err(|_| invalid(&format!("donor_init_root:{id}")))?;
    let payload = velnor_actions_tofu::tofu_payload_argv(TofuTaskKind::InitForValidate, &root)
        .map_err(|_| invalid(&format!("donor_init_argv:{id}")))?;
    let argv =
        velnor_actions_mise::IsolatedCommand::mise_exec(&[opentofu_spec(donor, id)?], &payload)
            .map_err(|_| invalid(&format!("donor_init_command:{id}")))?
            .argv()
            .into_iter()
            .map(std::ffi::OsString::into_string)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid(&format!("donor_init_encoding:{id}")))?;
    let scrub = crate::toolchain_env::credential_scrub();
    let env = env
        .iter()
        .filter(|(name, _)| !name.starts_with("VELNOR_") && !scrub.contains_key(*name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    let mut step = crate::steps::shell_step(FETCH_STEP_NAME, argv, env)?;
    step.condition = Some(TOFU_PROVIDERS_PROBE_MISS_CONDITION.to_owned());
    step.role = Some(StepRole::TofuProviderUse);
    Ok(step)
}

fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("tofu_providers_producer_{problem}"))
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::crate_job::TOOLS_PRODUCER_JOB_ID_PREFIX;
use velnor_actions_contract::workflow::{
    DEPENDENCY_SEED_JOB_CONDITION, TOOLS_PRODUCER_INSTALL_CONDITION,
};
use velnor_actions_contract::{Job, PullRequestCachePolicy, Step, StepRole};

use crate::RenderError;
use crate::tools_identity::{ToolsDigest, tools_digest};

const PRODUCER_DISPLAY_PREFIX: &str = "Tools producer / ";
const PREPARE_STEP_NAME: &str = "Prepare pinned tools";

struct ToolsProducerPlan {
    id: String,
    donor: String,
    digest: ToolsDigest,
}

enum ToolsInstall {
    DonorSteps,
    Specs(Vec<String>),
}

pub(crate) fn add_tools_producers(
    jobs: &mut BTreeMap<String, Job>,
    policy: PullRequestCachePolicy,
) -> Result<(), RenderError> {
    let mut built = Vec::new();
    for plan in plan(jobs)? {
        let donor = jobs
            .get(&plan.donor)
            .ok_or_else(|| invalid("donor_missing"))?;
        built.push((plan.id.clone(), build(&plan, donor, policy)?));
    }
    for (id, job) in built {
        if jobs.insert(id.clone(), job).is_some() {
            return Err(invalid(&format!("id_collision:{id}")));
        }
    }
    Ok(())
}

fn plan(jobs: &BTreeMap<String, Job>) -> Result<Vec<ToolsProducerPlan>, RenderError> {
    let mut donors: BTreeMap<ToolsDigest, &str> = BTreeMap::new();
    for (id, job) in jobs {
        if job.producer_kind().is_some() {
            continue;
        }
        if let Some(digest) = tools_digest(id, job)? {
            donors.entry(digest).or_insert(id.as_str());
        }
    }
    let mut plans = Vec::with_capacity(donors.len());
    for (digest, donor) in donors {
        let id = format!("{TOOLS_PRODUCER_JOB_ID_PREFIX}{}", digest.short());
        if plans
            .iter()
            .any(|existing: &ToolsProducerPlan| existing.id == id)
        {
            return Err(invalid(&format!("id_collision:{id}")));
        }
        plans.push(ToolsProducerPlan {
            id,
            donor: donor.to_owned(),
            digest,
        });
    }
    Ok(plans)
}

fn build(
    plan: &ToolsProducerPlan,
    donor: &Job,
    policy: PullRequestCachePolicy,
) -> Result<Job, RenderError> {
    let mut steps = Vec::new();
    for role in [
        StepRole::Checkout,
        StepRole::MiseSetup,
        StepRole::ToolsCacheIdentity,
    ] {
        steps.push(donor_step(donor, role, &plan.id)?.clone());
    }
    steps.push(crate::cache_steps::tools_cache_probe_step(
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION,
    )?);
    steps.extend(install_steps(donor, &plan.id)?);
    steps.push(crate::cache_steps::tools_cache_save_step(
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION,
        Some(crate::cache_p08::tools_cache_save_condition(policy)),
    )?);
    Ok(Job {
        display_name: format!("{PRODUCER_DISPLAY_PREFIX}{}", plan.digest.short()),
        runs_on: donor.runs_on.clone(),
        check_runner: None,
        timeout_minutes: donor.timeout_minutes,
        needs: Vec::new(),
        condition: Some(DEPENDENCY_SEED_JOB_CONDITION.to_owned()),
        permissions: None,
        cache_mode: None,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
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

fn install_steps(donor: &Job, id: &str) -> Result<Vec<Step>, RenderError> {
    let install = if donor
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::PreparePinnedTools))
    {
        ToolsInstall::DonorSteps
    } else {
        ToolsInstall::Specs(crate::cache_p08::infer_job_tools(donor))
    };
    let mut steps = match install {
        ToolsInstall::DonorSteps => donor
            .steps
            .iter()
            .filter(|step| is_install_role(step.role))
            .cloned()
            .collect(),
        ToolsInstall::Specs(specs) => vec![specs_step(&specs, id)?],
    };
    for step in &mut steps {
        if step.condition.is_some() {
            return Err(invalid(&format!("install_step_conditional:{id}")));
        }
        step.condition = Some(TOOLS_PRODUCER_INSTALL_CONDITION.to_owned());
    }
    Ok(steps)
}

fn is_install_role(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::PreparePinnedTools
                | StepRole::PrepareRustComponents
                | StepRole::VerifyRustManifest
                | StepRole::VerifyJactionlint
        )
    )
}

fn specs_step(specs: &[String], id: &str) -> Result<Step, RenderError> {
    let command = velnor_actions_mise::IsolatedCommand::mise_install(specs)
        .map_err(|_| invalid(&format!("install_specs_missing:{id}")))?;
    let argv = command
        .argv()
        .into_iter()
        .map(std::ffi::OsString::into_string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid(&format!("install_argv_encoding:{id}")))?;
    let mut step = crate::steps::ambient_shell_step(PREPARE_STEP_NAME, argv, BTreeMap::new())?;
    step.role = Some(StepRole::PreparePinnedTools);
    Ok(step)
}

fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("tools_producer_{problem}"))
}

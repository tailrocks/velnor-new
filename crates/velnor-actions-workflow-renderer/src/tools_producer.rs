// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::crate_job::TOOLS_PRODUCER_JOB_ID_PREFIX;
use velnor_actions_contract::workflow::{
    CacheAccess, CacheWriter, TOOLS_PRODUCER_INSTALL_CONDITION, validate_tools_producer,
};
use velnor_actions_contract::workflow::{JACTIONLINT_VERIFIER_USES, RUST_VERIFIER_USES};
use velnor_actions_contract::{Job, PullRequestCachePolicy, Step, StepKind, StepRole};

use velnor_actions_mise::{RustSpecKind, rust_spec_kind};

use crate::RenderError;
use crate::native_verifiers::{
    VERIFY_JACTIONLINT_STEP, VERIFY_RESTORED_RUST_STEP, verify_jactionlint_step,
    verify_rust_manifest_step,
};
use crate::tools_identity::{ToolsDigest, tools_digest};

const PRODUCER_DISPLAY_PREFIX: &str = "Tools producer / ";
const PREPARE_STEP_NAME: &str = "Prepare pinned tools";

struct ToolsProducerPlan {
    id: String,
    donor: Job,
    digest: ToolsDigest,
}

pub(crate) fn add_tools_producers(
    jobs: &mut BTreeMap<String, Job>,
    policy: PullRequestCachePolicy,
    standalone_donors: &BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    let mut built = Vec::new();
    for plan in plan(jobs, standalone_donors)? {
        let job = build(&plan, policy)?;
        validate_tools_producer(&job, &plan.id).map_err(RenderError::Contract)?;
        built.push((plan.id.clone(), job));
    }
    for (id, job) in built {
        if jobs.insert(id.clone(), job).is_some() {
            return Err(invalid(&format!("id_collision:{id}")));
        }
    }
    Ok(())
}

fn plan(
    jobs: &BTreeMap<String, Job>,
    standalone_donors: &BTreeMap<String, Job>,
) -> Result<Vec<ToolsProducerPlan>, RenderError> {
    let mut donors: BTreeMap<ToolsDigest, &Job> = BTreeMap::new();
    for (id, job) in jobs.iter().chain(standalone_donors) {
        if job.producer_kind().is_some() {
            continue;
        }
        if let Some(digest) = tools_digest(id, job)? {
            donors.entry(digest).or_insert(job);
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
            donor: donor.clone(),
            digest,
        });
    }
    Ok(plans)
}

fn build(plan: &ToolsProducerPlan, policy: PullRequestCachePolicy) -> Result<Job, RenderError> {
    let donor = &plan.donor;
    let mut steps = vec![crate::state_seal::tip::stale_tip_stops_job_step()?];
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
    steps.push(gated(crate::cache_steps::tools_cache_scrub_step(
        &donor_mise_sha256(donor, &plan.id)?,
    )?));
    steps.extend(install_steps(donor, &plan.id)?);
    steps.push(crate::cache_steps::tools_cache_save_step(
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION,
        Some(crate::cache_p08::tools_cache_save_condition(policy)),
    )?);
    Ok(Job {
        display_name: format!("{PRODUCER_DISPLAY_PREFIX}{}", plan.digest.short()),
        runs_on: donor.runs_on.clone(),
        check_runner: None,
        timeout_minutes: crate::cache_save_budget::lift_to_save_budget(donor.timeout_minutes)?,
        needs: Vec::new(),
        condition: Some(CacheWriter::ToolsProducer.condition()),
        permissions: None,
        cache_access: CacheAccess::Writer(CacheWriter::ToolsProducer),
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

fn donor_mise_sha256(donor: &Job, id: &str) -> Result<String, RenderError> {
    match &donor_step(donor, StepRole::ToolsCacheRestore, id)?.kind {
        StepKind::Action { with, .. } => with
            .get(crate::cache_steps::TOOLS_MISE_SHA256_INPUT)
            .cloned()
            .ok_or_else(|| invalid(&format!("donor_mise_sha256_missing:{id}"))),
        _ => Err(invalid(&format!("donor_mise_sha256_missing:{id}"))),
    }
}

fn gated(mut step: Step) -> Step {
    step.condition = Some(TOOLS_PRODUCER_INSTALL_CONDITION.to_owned());
    step
}

fn install_steps(donor: &Job, id: &str) -> Result<Vec<Step>, RenderError> {
    let mut role_steps = Vec::new();
    for step in donor.steps.iter().filter(|step| is_install_role(step.role)) {
        if let Some(step) = producer_form(step, id)? {
            role_steps.push(step);
        }
    }
    let digest_specs = crate::cache_p08::infer_job_tools(donor);
    if digest_specs.iter().any(|spec| {
        matches!(
            rust_spec_kind(spec),
            RustSpecKind::Plain(_) | RustSpecKind::UnsupportedRust
        )
    }) {
        return Err(invalid(&format!("donor_rust_install_not_minimal:{id}")));
    }
    let has_role = |role| role_steps.iter().any(|step| step.role == Some(role));
    let mut steps = if has_role(StepRole::PreparePinnedTools) {
        let donor_role_steps = donor.steps.iter().filter(|step| is_install_role(step.role));
        if crate::cache_p08::infer_steps_tools(donor_role_steps) != digest_specs {
            return Err(invalid(&format!("donor_inline_install:{id}")));
        }
        role_steps
    } else {
        if has_role(StepRole::PrepareRustComponents) {
            return Err(invalid(&format!("donor_components_without_prepare:{id}")));
        }
        let rust = digest_specs
            .iter()
            .any(|spec| velnor_actions_mise::rust_spec_version(spec).is_some());
        if rust && !has_role(StepRole::VerifyRustManifest) {
            return Err(invalid(&format!("donor_rust_without_verifier:{id}")));
        }
        let mut steps = vec![specs_step(&digest_specs, id)?];
        steps.extend(role_steps);
        steps
    };
    for step in &mut steps {
        if step.condition.is_some() {
            return Err(invalid(&format!("install_step_conditional:{id}")));
        }
        step.condition = Some(TOOLS_PRODUCER_INSTALL_CONDITION.to_owned());
    }
    Ok(steps)
}

fn producer_form(step: &Step, id: &str) -> Result<Option<Step>, RenderError> {
    let action =
        |uses: &str| matches!(&step.kind, StepKind::Action { uses: have, .. } if have == uses);
    match step.role {
        Some(StepRole::VerifyRustManifest) => {
            if action(RUST_VERIFIER_USES[0]) || step.name == VERIFY_RESTORED_RUST_STEP {
                Ok(None)
            } else if action(RUST_VERIFIER_USES[1]) {
                Ok(Some(step.clone()))
            } else if step.name == crate::VERIFY_RUST_TOOLCHAIN_STEP {
                Ok(Some(verify_rust_manifest_step()))
            } else {
                Err(invalid(&format!("donor_unknown_verifier:{id}")))
            }
        }
        Some(StepRole::VerifyJactionlint) => {
            if action(JACTIONLINT_VERIFIER_USES) {
                Ok(Some(step.clone()))
            } else if step.name == VERIFY_JACTIONLINT_STEP {
                Ok(Some(verify_jactionlint_step()))
            } else {
                Err(invalid(&format!("donor_unknown_verifier:{id}")))
            }
        }
        _ => Ok(Some(step.clone())),
    }
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

pub(crate) fn specs_step(specs: &[String], id: &str) -> Result<Step, RenderError> {
    let command = velnor_actions_mise::IsolatedCommand::mise_install(specs)
        .map_err(|_| invalid(&format!("install_specs_missing:{id}")))?;
    let argv = command
        .argv()
        .into_iter()
        .map(std::ffi::OsString::into_string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid(&format!("install_argv_encoding:{id}")))?;
    let env = if specs
        .iter()
        .any(|spec| velnor_actions_mise::rust_spec_version(spec).is_some())
    {
        velnor_actions_contract::HomeKey::ALL
            .iter()
            .map(|key| (key.env_name().to_owned(), key.standard_value().to_owned()))
            .collect()
    } else {
        BTreeMap::new()
    };
    let mut step = crate::steps::ambient_shell_step(PREPARE_STEP_NAME, argv, env)?;
    step.role = Some(StepRole::PreparePinnedTools);
    Ok(step)
}

fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("tools_producer_{problem}"))
}

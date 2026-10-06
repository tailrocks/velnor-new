//! Separate planning and full tool cache domains around authenticated analysis.
use crate::{MiseSetup, RenderError};
use velnor_actions_contract::{Job, Step, StepId, StepKind};
#[path = "cache_tool_phase_validate.rs"]
mod validate;
use validate::{reject_foreign_transports, validate_existing};

pub(crate) const PLANNING_ROOT: &str = "${{ runner.temp }}/velnor/planning/mise";
pub(crate) const PLANNING_RESTORE_ID: &str = "velnor-planning-tools-cache";
const PLANNING_RESTORE_NAME: &str = "Restore planning tools";

/// Closed transport identity: each domain owns one immutable canonical payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ToolDomain {
    Planning,
    Full,
}

impl ToolDomain {
    fn restore_name(self) -> &'static str {
        match self {
            Self::Planning => PLANNING_RESTORE_NAME,
            Self::Full => crate::cache_steps::TOOLS_RESTORE_NAME,
        }
    }
    fn restore_id(self) -> &'static str {
        match self {
            Self::Planning => PLANNING_RESTORE_ID,
            Self::Full => crate::cache_steps::TOOLS_RESTORE_ID,
        }
    }
    fn paths(self) -> Vec<String> {
        match self {
            Self::Planning => velnor_actions_contract::ToolCacheDomain::Planning,
            Self::Full => velnor_actions_contract::ToolCacheDomain::Full,
        }
        .payload()
    }
    fn require(self, step: &mut Step) {
        if self == Self::Full {
            crate::early_plan::require_cargo(step);
        }
    }
}

fn boundary(job: &Job) -> Result<usize, RenderError> {
    let present: Vec<_> = job.steps.iter().enumerate().filter(|(_, step)| {
        step.id.as_ref().is_some_and(|id| id.as_str() == crate::early_plan::EARLY_PLAN_STEP_ID)
            || matches!(&step.kind, StepKind::Internal { operation } if operation == crate::steps::EARLY_PLAN_OPERATION)
    }).collect();
    if let [(at, step)] = present.as_slice()
        && step
            .id
            .as_ref()
            .is_some_and(|id| id.as_str() == crate::early_plan::EARLY_PLAN_STEP_ID)
        && matches!(&step.kind, StepKind::Internal { operation } if operation == crate::steps::EARLY_PLAN_OPERATION)
    {
        if **step == crate::early_plan::early_plan_step()? {
            return Ok(*at);
        }
    }
    Err(invalid("early_plan_binding_missing_or_duplicate"))
}

fn validate_planner_operations(job: &Job) -> Result<(), RenderError> {
    let early = boundary(job)?;
    let plans: Vec<_> = job.steps.iter().enumerate().filter(|(_, step)| {
        matches!(&step.kind, StepKind::Internal { operation } if operation == crate::steps::PLAN_OPERATION)
    }).map(|(at, _)| at).collect();
    if let [at] = plans.as_slice()
        && *at > early
        && job.steps[*at] == crate::steps::plan_step()
    {
        return Ok(());
    }
    Err(invalid(
        "phased_tools_planner_missing_duplicate_or_misordered",
    ))
}

/// Bootstrap planning tools independently; full transfer starts only on a miss.
pub(super) fn ensure(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    target: &str,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<(), RenderError> {
    super::validate_job_tool_records(job, records)?;
    validate_planner_operations(job)?;
    let (planning_key, full_key) = phase_keys(job, setup, target, records)?;
    let bootstrap = crate::setup::mise_setup_step(
        setup,
        velnor_actions_contract::ToolCacheDomain::Planning,
        &job.runs_on,
    )?;
    let planning_restore = restore(ToolDomain::Planning, &planning_key)?;
    let full_restore = restore(ToolDomain::Full, &full_key)?;
    let present: Vec<_> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| super::is_setup_step(step))
        .map(|(at, _)| at)
        .collect();
    match present.len() {
        0 => {}
        2 => {
            return validate_existing(
                job_id,
                job,
                setup,
                &bootstrap,
                &planning_restore,
                &full_restore,
            );
        }
        _ => return Err(invalid("phased_tools_duplicate_setup")),
    }
    insert_controls(
        job,
        setup,
        bootstrap.clone(),
        planning_restore.clone(),
        full_restore.clone(),
    )?;
    validate_existing(
        job_id,
        job,
        setup,
        &bootstrap,
        &planning_restore,
        &full_restore,
    )
}

fn insert_controls(
    job: &mut Job,
    setup: &MiseSetup,
    bootstrap: Step,
    planning_restore: Step,
    full_restore: Step,
) -> Result<(), RenderError> {
    reject_foreign_transports(job)?;
    let mut platform = super::payload::platform_step()?;
    planning_env(&mut platform);
    let at = job
        .steps
        .iter()
        .position(|step| step.name == crate::steps::ACQUIRE_NAME)
        .map_or_else(
            || {
                usize::from(
                    job.steps
                        .first()
                        .is_some_and(|step| step.name == "Checkout"),
                )
            },
            |index| index + 1,
        )
        .min(job.steps.len());
    drop(
        job.steps
            .splice(at..at, [platform, planning_restore, bootstrap]),
    );
    let early_at = boundary(job)?;
    let full_pin = setup.bootstrap(velnor_actions_contract::ToolCacheDomain::Full, &job.runs_on)?;
    let mut full_preflight = super::payload::preflight::preflight_step(
        full_pin,
        velnor_actions_contract::ToolCacheDomain::Full,
    )?;
    if let StepKind::Shell { env, .. } = &mut full_preflight.kind {
        env.insert(
            "MISE_DATA_DIR".to_owned(),
            velnor_actions_contract::ToolCacheDomain::Full
                .root()
                .to_owned(),
        );
    }
    ToolDomain::Full.require(&mut full_preflight);
    let full_bootstrap = full_bootstrap(job, setup)?;
    drop(job.steps.splice(
        (early_at + 1)..=early_at,
        [full_restore, full_preflight, full_bootstrap],
    ));
    for (at, step) in job.steps.iter_mut().enumerate() {
        if let StepKind::Shell { env, .. } = &mut step.kind {
            env.insert(
                "MISE_DATA_DIR".to_owned(),
                if at <= early_at {
                    PLANNING_ROOT
                } else {
                    crate::cache_steps::TOOLS_CACHE_PATH
                }
                .to_owned(),
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_prefix(
    job: &Job,
    setup: &MiseSetup,
    target: &str,
    ctx: &crate::RenderContext,
) -> Result<(), RenderError> {
    let (planning_key, _) = phase_keys(job, setup, target, &ctx.source_helpers)?;
    let planning = restore(ToolDomain::Planning, &planning_key)?;
    let mut platform = super::payload::platform_step()?;
    planning_env(&mut platform);
    let bootstrap = crate::setup::mise_setup_step(
        setup,
        velnor_actions_contract::ToolCacheDomain::Planning,
        &job.runs_on,
    )?;
    crate::early_prefix_admission::validate(
        job,
        &ctx.staged_binary,
        &ctx.checkout_uses,
        &platform,
        &planning,
        &bootstrap,
        &ctx.source_helpers,
    )
}

fn full_bootstrap(job: &Job, setup: &MiseSetup) -> Result<Step, RenderError> {
    let mut step = crate::setup::mise_setup_step(
        setup,
        velnor_actions_contract::ToolCacheDomain::Full,
        &job.runs_on,
    )?;
    ToolDomain::Full.require(&mut step);
    Ok(step)
}

fn phase_keys(
    job: &Job,
    setup: &MiseSetup,
    target: &str,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<(String, String), RenderError> {
    let early_at = boundary(job)?;
    let mut prefix = job.clone();
    prefix.steps.truncate(early_at);
    let planning_specs = super::infer_job_tools(&prefix);
    if planning_specs.is_empty() {
        return Err(invalid("early_plan_without_planning_tools"));
    }
    let planning_key = domain_key(ToolDomain::Planning, target, setup, &prefix, records)?;
    let mut suffix = job.clone();
    drop(suffix.steps.drain(..=early_at));
    let full_key = domain_key(ToolDomain::Full, target, setup, &suffix, records)?;
    Ok((planning_key, full_key))
}

fn planning_env(step: &mut Step) {
    if let StepKind::Shell { env, .. } = &mut step.kind {
        env.insert("MISE_DATA_DIR".to_owned(), PLANNING_ROOT.to_owned());
    }
}

fn domain_key(
    domain: ToolDomain,
    target: &str,
    setup: &MiseSetup,
    job: &Job,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<String, RenderError> {
    let key = if domain == ToolDomain::Full && super::infer_job_tools(job).is_empty() {
        super::mise_cache_key_for_tools(target, &setup.version, &["mise@bootstrap".to_owned()])?
    } else {
        super::mise_cache_key_for_job(target, &setup.version, job, records)?
    };
    let key = if domain == ToolDomain::Planning {
        key.replacen(
            &format!("{}-", super::MISE_KEY_PREFIX),
            &format!("{}-planning-", super::MISE_KEY_PREFIX),
            1,
        )
    } else {
        key
    };
    Ok(format!("{key}-${{{{env.VELNOR_CACHE_IMAGE}}}}"))
}

fn restore(domain: ToolDomain, key: &str) -> Result<Step, RenderError> {
    let lookup = format!("{key}-lookup-${{{{github.run_id}}}}-${{{{github.run_attempt}}}}");
    let mut step = crate::cache_steps::cache_action_step(
        true,
        crate::cache_steps::TOOLS_RESTORE_USES,
        "tools",
        &lookup,
        &[format!("{key}-snapshot-")],
        &domain.paths(),
    )?;
    domain.restore_name().clone_into(&mut step.name);
    step.id = Some(StepId::new(domain.restore_id()).map_err(RenderError::Contract)?);
    domain.require(&mut step);
    Ok(step)
}

fn invalid(reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(reason.to_owned())
}

#[cfg(test)]
#[path = "cache_tool_phases_tests.rs"]
mod tests;

//! Fail closed on changed or misordered phased cache controls.
use super::super::{detector_words, payload};
use super::{
    Job, MiseSetup, PLANNING_RESTORE_ID, PLANNING_ROOT, RenderError, Step, StepKind, ToolDomain,
    boundary, full_bootstrap, invalid, planning_env,
};

pub(super) fn reject_foreign_transports(job: &Job) -> Result<(), RenderError> {
    if job.steps.iter().any(owned_transport) {
        return Err(invalid("early_tools_foreign_transport"));
    }
    Ok(())
}

pub(super) fn validate_existing(
    job_id: &str,
    job: &Job,
    pins: &MiseSetup,
    setup: &Step,
    planning: &Step,
    full: &Step,
) -> Result<(), RenderError> {
    let full_bootstrap = full_bootstrap(job, pins)?;
    let expected = [setup, &full_bootstrap, planning, full];
    for step in expected {
        let matches: Vec<_> = job
            .steps
            .iter()
            .enumerate()
            .filter(|(_, item)| *item == step)
            .collect();
        if !matches!(matches.as_slice(), [(_, item)] if *item == step) {
            return Err(invalid(&format!(
                "phased_tools_changed:{job_id}:{}",
                step.name
            )));
        }
    }
    let index = |name: &str| {
        job.steps
            .iter()
            .position(|step| step.name == name)
            .unwrap_or(usize::MAX)
    };
    validate_auxiliary(job, pins, setup, full)?;
    validate_domains(job, setup, planning, full)?;
    validate_readonly_transports(job)?;
    let early = boundary(job)?;
    let plan = job
        .steps
        .iter()
        .position(|step| {
            matches!(&step.kind, StepKind::Internal { operation }
        if operation == crate::steps::PLAN_OPERATION)
        })
        .ok_or_else(|| invalid("phased_tools_missing_plan"))?;
    let full_preflight = job
        .steps
        .iter()
        .position(|step| {
            matches!(&step.kind,
        StepKind::Shell { run, env } if payload::preflight::is_preflight_argv(run)
            && env.get("VELNOR_MISE_CACHE_DOMAIN").map(String::as_str) == Some("tools"))
        })
        .ok_or_else(|| invalid("phased_tools_missing_full_preflight"))?;
    let full_bootstrap_at = job
        .steps
        .iter()
        .position(|step| step == &full_bootstrap)
        .ok_or_else(|| invalid("phased_tools_missing_full_bootstrap"))?;
    if index(full.name.as_str()) >= plan || full_preflight >= plan || full_bootstrap_at >= plan {
        return Err(invalid("phased_tools_plan_before_restore"));
    }
    if index(planning.name.as_str()) >= index(setup.name.as_str())
        || index(setup.name.as_str()) >= early
        || index(full.name.as_str()) <= early
    {
        return Err(invalid("phased_tools_misordered"));
    }
    Ok(())
}

fn validate_auxiliary(
    job: &Job,
    pins: &MiseSetup,
    setup: &Step,
    full: &Step,
) -> Result<(), RenderError> {
    let pin = pins.bootstrap(velnor_actions_contract::ToolCacheDomain::Full, &job.runs_on)?;
    let mut full_preflight =
        payload::preflight::preflight_step(pin, velnor_actions_contract::ToolCacheDomain::Full)?;
    if let StepKind::Shell { env, .. } = &mut full_preflight.kind {
        env.insert(
            "MISE_DATA_DIR".to_owned(),
            velnor_actions_contract::ToolCacheDomain::Full
                .root()
                .to_owned(),
        );
    }
    ToolDomain::Full.require(&mut full_preflight);
    let preflights: Vec<_> = job
        .steps
        .iter()
        .filter(|step| {
            matches!(&step.kind,
        StepKind::Shell { run, .. } if payload::preflight::is_preflight_argv(run))
        })
        .collect();
    if preflights != [&full_preflight] {
        return Err(invalid("phased_tools_preflight_changed"));
    }

    let index = |expected: &Step| {
        job.steps
            .iter()
            .position(|step| step == expected)
            .unwrap_or(usize::MAX)
    };
    let planning_restore = job
        .steps
        .iter()
        .find(|step| {
            step.id
                .as_ref()
                .is_some_and(|id| id.as_str() == PLANNING_RESTORE_ID)
        })
        .ok_or_else(|| invalid("phased_tools_missing_planning_restore"))?;
    let full_bootstrap = full_bootstrap(job, pins)?;
    if index(planning_restore) + 1 != index(setup)
        || index(&full_preflight) != index(full) + 1
        || index(&full_bootstrap) != index(&full_preflight) + 1
    {
        return Err(invalid("phased_tools_preflight_misordered"));
    }
    let first_full_use = job.steps.iter().enumerate().skip(index(full) + 1).find_map(|(at, step)| {
        matches!(&step.kind, StepKind::Shell { run, .. } if detector_words(run).iter().any(|word| crate::cache_tool_paths::is_mise_word(word))).then_some(at)
    }).unwrap_or(usize::MAX);
    if index(&full_preflight) > first_full_use {
        return Err(invalid("phased_tools_preflight_after_use"));
    }
    let mut platform = payload::platform_step()?;
    planning_env(&mut platform);
    let platforms: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.name == platform.name)
        .collect();
    if platforms != [&platform] || index(&platform) >= index(planning_restore) {
        return Err(invalid("phased_tools_platform_changed"));
    }
    let restores: Vec<_> = job
        .steps
        .iter()
        .filter(|step| owned_transport(step) && matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/cache/restore@")))
        .collect();
    if restores.len() != 2 || !restores.contains(&full) {
        return Err(invalid("phased_tools_foreign_restore"));
    }
    Ok(())
}

use crate::cache_tool_paths::owned_transport;

fn validate_domains(
    job: &Job,
    setup: &Step,
    planning: &Step,
    full: &Step,
) -> Result<(), RenderError> {
    let index = |expected: &Step| {
        job.steps
            .iter()
            .position(|step| step == expected)
            .unwrap_or(usize::MAX)
    };
    let early = boundary(job)?;
    let full_preflight = job.steps.iter().position(|step| matches!(&step.kind, StepKind::Shell { run, env }
        if payload::preflight::is_preflight_argv(run) && env.get("VELNOR_MISE_CACHE_DOMAIN").map(String::as_str) == Some("tools")))
        .ok_or_else(|| invalid("phased_tools_missing_full_preflight"))?;
    let full_bootstrap = job
        .steps
        .iter()
        .position(|step| {
            super::super::is_setup_step(step)
                && step.condition.as_deref() == Some(crate::early_plan::NEEDS_CARGO_CONDITION)
        })
        .ok_or_else(|| invalid("phased_tools_missing_full_bootstrap"))?;
    for (at, step) in job.steps.iter().enumerate() {
        let (uses_mise, env) = match &step.kind {
            StepKind::Shell { run, env } => (shell_uses_mise(run, env, at <= early)?, env),
            StepKind::SourceBoundHelper { invocation, env } => (
                invocation.descriptor().operation()
                    != velnor_actions_contract::SourceBoundOperation::MiseBootstrap
                    && (!invocation.installed_selectors().is_empty()
                        || env.contains_key("VELNOR_TOOL_CACHE_IDENTITY")
                        || env.contains_key("VELNOR_RUSTUP_IDENTITY")
                        || env.contains_key("VELNOR_QUALIFIED_TOOL_IDENTITY")),
                env,
            ),
            _ => continue,
        };
        let root = if at <= early {
            PLANNING_ROOT
        } else {
            crate::cache_steps::TOOLS_CACHE_PATH
        };
        if (uses_mise || env.contains_key("MISE_DATA_DIR"))
            && env.get("MISE_DATA_DIR").map(String::as_str) != Some(root)
        {
            return Err(invalid("phased_tools_shell_domain_changed"));
        }
        if at > early
            && (matches!(&step.kind, StepKind::Shell { .. })
                || uses_mise
                || env.contains_key("MISE_DATA_DIR"))
            && step.condition.as_deref() != Some(crate::early_plan::NEEDS_CARGO_CONDITION)
        {
            return Err(invalid("phased_tools_full_use_without_cargo_guard"));
        }
        let full_shell = at > early
            && matches!(&step.kind, StepKind::Shell { .. })
            && !matches!(&step.kind, StepKind::Shell { run, .. } if payload::preflight::is_preflight_argv(run));
        if (uses_mise || full_shell)
            && (at <= early && (at <= index(planning) || at <= index(setup))
                || at > early
                    && (at <= index(full) || at <= full_preflight || at <= full_bootstrap))
        {
            return Err(invalid("phased_tools_use_before_restore"));
        }
    }
    Ok(())
}

fn validate_readonly_transports(job: &Job) -> Result<(), RenderError> {
    if job.steps.iter().any(|step| owned_transport(step)
        && matches!(&step.kind, StepKind::Action { uses, .. } if !uses.starts_with("actions/cache/restore@"))) {
        return Err(invalid("phased_tools_consumer_write"));
    }
    Ok(())
}

fn shell_uses_mise(
    run: &[String],
    env: &std::collections::BTreeMap<String, String>,
    planning: bool,
) -> Result<bool, RenderError> {
    let expected = if planning {
        velnor_actions_contract::ToolCacheDomain::Planning
    } else {
        velnor_actions_contract::ToolCacheDomain::Full
    };
    let words = detector_words(run);
    if words.iter().any(|word| {
        crate::cache_tool_paths::mise_word_domain(
            word,
            env.get("MISE_DATA_DIR").map(String::as_str),
        )
        .is_some_and(|domain| domain != expected)
    }) {
        return Err(invalid("phased_tools_executable_domain_changed"));
    }
    Ok(words
        .iter()
        .any(|word| crate::cache_tool_paths::is_mise_word(word)))
}

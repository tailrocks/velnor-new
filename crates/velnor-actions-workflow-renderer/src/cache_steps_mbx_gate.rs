//! Validation that generated MBX steps match each job's selected compiler.

use std::collections::BTreeMap;

use super::mbx_command::{has_external_mbx_selector, mbx_selector_surfaces, uses_mbx_command};
use super::mbx_preflight::{
    canonical_mbx_preflight_step, mbx_install_action_step, mbx_version_check_step,
};
use super::{CompileDriver, MBX_ACTION_NAME};
use crate::RenderError;
use velnor_actions_contract::{Job, Step, StepKind, StepRole};

/// Gate native MBX action and executable selection against each job's driver.
///
/// Jobs without a declared driver are skipped (plan/final/lint carry none);
/// Cargo jobs are MBX-free while MBX jobs carry exactly one action and command.
/// The one allowed mise surface is the pinned-tools preflight installing the
/// job's own action MBX version (the repository `mr_boxington = true` rust
/// record makes mise's exec-time wrapper dispatch cargo through the
/// mise-managed tool, so the exact action pin must also be installed there).
/// # Errors
pub fn check_mbx_gating(
    jobs: &BTreeMap<String, Job>,
    drivers: &BTreeMap<String, CompileDriver>,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        for step in &job.steps {
            if !has_external_mbx_selector(step) {
                continue;
            }
            if installs_only_the_action_mbx_version(step, job) {
                continue;
            }
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_mise_selector_forbidden:{id}"
            )));
        }
    }
    for (id, driver) in drivers {
        let Some(job) = jobs.get(id.as_str()) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_gating_unknown_job:{id}"
            )));
        };
        check_job_mbx(id, job, *driver)?;
    }
    for (id, job) in jobs {
        if job
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::MbxPreflight))
            && drivers.get(id) != Some(&CompileDriver::Mbx)
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_preflight_without_selection:{id}"
            )));
        }
    }
    Ok(())
}

/// Whether the step's only MBX selectors are the pinned-tools install of
/// the job's own native action version.
///
/// The repository rust record's `mr_boxington = true` makes mise's
/// exec-time wrapper dispatch cargo through the mise-managed
/// `mr-boxington` tool, so `Prepare pinned tools` installs the exact pin
/// the job's native MBX action also carries: one version, two transports.
/// Any other surface — another version, another role, an `exec` selector —
/// stays forbidden.
fn installs_only_the_action_mbx_version(step: &Step, job: &Job) -> bool {
    if step.role != Some(StepRole::PreparePinnedTools) {
        return false;
    }
    let (install, exec) = mbx_selector_surfaces(step);
    if install.is_empty() || !exec.is_empty() {
        return false;
    }
    let mut versions = job.steps.iter().filter_map(mbx_action_version);
    let Some(version) = versions.next() else {
        return false;
    };
    versions.next().is_none()
        && install
            .iter()
            .all(|selector| selector == &format!("mr-boxington@{version}"))
}

/// The `version` input of the job's native MBX action, if it is one.
fn mbx_action_version(step: &Step) -> Option<&str> {
    if !is_mbx_action(step) {
        return None;
    }
    match &step.kind {
        StepKind::Action { with, .. } => with.get("version").map(String::as_str),
        _ => None,
    }
}

fn check_job_mbx(id: &str, job: &Job, driver: CompileDriver) -> Result<(), RenderError> {
    let actions: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_mbx_action(step))
        .map(|(index, _)| index)
        .collect();
    let all_commands: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| uses_mbx_command(step) && !is_mbx_action(step))
        .map(|(index, _)| index)
        .collect();
    let build_commands: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            step.role != Some(StepRole::MbxVersionCheck)
                && uses_mbx_command(step)
                && !is_mbx_action(step)
        })
        .map(|(index, _)| index)
        .collect();
    let version_checks: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(StepRole::MbxVersionCheck))
        .map(|(index, _)| index)
        .collect();
    let preflights: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(StepRole::MbxPreflight))
        .map(|(index, _)| index)
        .collect();
    match driver {
        CompileDriver::Cargo if !actions.is_empty() => Err(RenderError::InvalidWorkflow(format!(
            "mbx_action_without_selection:{id}"
        ))),
        CompileDriver::Cargo if !all_commands.is_empty() => Err(RenderError::InvalidWorkflow(
            format!("mbx_tool_without_selection:{id}"),
        )),
        CompileDriver::Cargo => Ok(()),
        CompileDriver::Mbx if actions.is_empty() => Err(RenderError::InvalidWorkflow(format!(
            "mbx_missing_for_selection:{id}"
        ))),
        CompileDriver::Mbx if actions.len() > 1 => {
            Err(RenderError::InvalidWorkflow(format!("mbx_duplicated:{id}")))
        }
        CompileDriver::Mbx if job.steps[actions[0]].role != Some(StepRole::MbxInstall) => Err(
            RenderError::InvalidWorkflow(format!("mbx_action_role_missing:{id}")),
        ),
        CompileDriver::Mbx if preflights.is_empty() => Err(RenderError::InvalidWorkflow(format!(
            "mbx_preflight_missing:{id}"
        ))),
        CompileDriver::Mbx if preflights.len() > 1 => Err(RenderError::InvalidWorkflow(format!(
            "mbx_preflight_duplicated:{id}"
        ))),
        CompileDriver::Mbx if version_checks.is_empty() => Err(RenderError::InvalidWorkflow(
            format!("mbx_version_check_missing:{id}"),
        )),
        CompileDriver::Mbx if version_checks.len() > 1 => Err(RenderError::InvalidWorkflow(
            format!("mbx_version_check_duplicated:{id}"),
        )),
        CompileDriver::Mbx => check_mbx_version_order(
            id,
            job,
            preflights[0],
            actions[0],
            &build_commands,
            version_checks[0],
        ),
    }
}

fn check_mbx_version_order(
    id: &str,
    job: &Job,
    preflight_at: usize,
    action_at: usize,
    commands: &[usize],
    check_at: usize,
) -> Result<(), RenderError> {
    let Some(action) = job.steps.get(action_at) else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_action_missing:{id}"
        )));
    };
    let velnor_actions_contract::StepKind::Action { uses, with, env } = &action.kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_action_invalid:{id}"
        )));
    };
    let Some(version) = with.get("version") else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_version_missing:{id}"
        )));
    };
    let expected_action = mbx_install_action_step(uses, version, env.clone())?;
    if !same(action, &expected_action) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_action_not_install_only:{id}"
        )));
    }
    let rust_toolchain = job
        .steps
        .get(check_at)
        .and_then(|step| match &step.kind {
            StepKind::Shell { run, .. } => crate::action_mbx_shell::script_rust_toolchain(run),
            _ => None,
        })
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("mbx_toolchain_missing:{id}")))?;
    let expected_preflight = canonical_mbx_preflight_step(version, rust_toolchain, env)?;
    if job.steps.get(preflight_at) != Some(&expected_preflight) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_preflight_mismatch:{id}"
        )));
    }
    let expected = mbx_version_check_step(version, rust_toolchain, env.clone())?;
    if job.steps.get(check_at) != Some(&expected) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_version_check_mismatch:{id}"
        )));
    }
    if preflight_at >= action_at
        || action_at >= check_at
        || commands.is_empty()
        || commands.iter().any(|index| *index <= check_at)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_version_check_order:{id}"
        )));
    }
    Ok(())
}

/// Validate every job's native MBX lifecycle, with or without a declared driver.
///
/// A job that carries any MBX lifecycle step carries exactly one canonical
/// preflight, install-only action, and version check, in that order, and
/// no MBX consumer runs before the version check.
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] naming the job and the broken rule.
pub(crate) fn validate_mbx_lifecycles(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    for (id, job) in jobs {
        validate_mbx_lifecycle(id, job)?;
    }
    Ok(())
}

fn validate_mbx_lifecycle(id: &str, job: &Job) -> Result<(), RenderError> {
    let actions = role_indices(job, StepRole::MbxInstall);
    let ready = role_indices(job, StepRole::MbxVersionCheck);
    let preflights = role_indices(job, StepRole::MbxPreflight);
    if actions.is_empty() && ready.is_empty() && preflights.is_empty() {
        return Ok(());
    }
    let problem = match (actions.as_slice(), ready.as_slice(), preflights.as_slice()) {
        ([], _, _) => Some("mbx_lifecycle_without_action"),
        ([_, _, ..], _, _) => Some("mbx_action_duplicated"),
        (_, [], _) => Some("mbx_ready_check_missing"),
        (_, [_, _, ..], _) => Some("mbx_ready_check_duplicated"),
        (_, _, [_, _, ..]) => Some("mbx_preflight_duplicated"),
        ([action], [check], _) if action >= check => Some("mbx_ready_check_order"),
        _ => None,
    };
    if let Some(problem) = problem {
        return Err(RenderError::InvalidWorkflow(format!("{problem}:{id}")));
    }
    let (action_at, ready_at) = (actions[0], ready[0]);
    let StepKind::Action { uses, with, env } = &job.steps[action_at].kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_action_kind_mismatch:{id}"
        )));
    };
    let version = with
        .get("version")
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("mbx_version_missing:{id}")))?;
    if !same(
        &job.steps[action_at],
        &mbx_install_action_step(uses, version, env.clone())?,
    ) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_action_not_install_only:{id}"
        )));
    }
    let StepKind::Shell { run, .. } = &job.steps[ready_at].kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_ready_check_mismatch:{id}"
        )));
    };
    let rust = crate::action_mbx_shell::script_rust_toolchain(run)
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("mbx_ready_check_mismatch:{id}")))?;
    if !same(
        &job.steps[ready_at],
        &mbx_version_check_step(version, rust, env.clone())?,
    ) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_ready_check_mismatch:{id}"
        )));
    }
    if let Some(&preflight_at) = preflights.first()
        && (preflight_at >= action_at
            || !same(
                &job.steps[preflight_at],
                &canonical_mbx_preflight_step(version, rust, env)?,
            ))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_preflight_mismatch:{id}"
        )));
    }
    let consumer_before_ready = job.steps.iter().take(ready_at).any(|step| {
        step.role != Some(StepRole::MbxVersionCheck)
            && uses_mbx_command(step)
            && !is_mbx_action(step)
    });
    if consumer_before_ready {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_consumer_before_ready:{id}"
        )));
    }
    Ok(())
}

/// Display names never carry authority: compare everything else.
fn same(actual: &Step, expected: &Step) -> bool {
    actual.parallel == expected.parallel && crate::cache_p08::same_step_semantics(actual, expected)
}

fn role_indices(job: &Job, role: StepRole) -> Vec<usize> {
    job.steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(role))
        .map(|(index, _)| index)
        .collect()
}

/// True for the pinned native MBX action.
pub(crate) fn is_mbx_action(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Action { uses, .. } if uses.starts_with(&format!("{MBX_ACTION_NAME}@")))
}

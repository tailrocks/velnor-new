//! Validation that generated MBX steps match each job's selected compiler.

use std::collections::BTreeMap;

use super::mbx_command::{has_external_mbx_selector, uses_mbx_command};
use super::mbx_preflight::{canonical_mbx_preflight_step, mbx_version_check_step};
use super::{CompileDriver, MBX_ACTION_NAME};
use crate::RenderError;
use velnor_actions_contract::{Job, Step, StepKind, StepRole};

/// Gate native MBX action and executable selection against each job's driver.
///
/// Jobs without a declared driver are skipped (plan/final/lint carry none);
/// Cargo jobs are MBX-free while MBX jobs carry exactly one action and command.
/// # Errors
pub fn check_mbx_gating(
    jobs: &BTreeMap<String, Job>,
    drivers: &BTreeMap<String, CompileDriver>,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        if job.steps.iter().any(has_external_mbx_selector) {
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
        CompileDriver::Mbx if job.steps[actions[0]].role != Some(StepRole::MbxCache) => Err(
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
    let velnor_actions_contract::StepKind::Action { with, env, .. } = &action.kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_action_invalid:{id}"
        )));
    };
    let Some(version) = with.get("version") else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_version_missing:{id}"
        )));
    };
    let Some(rust_toolchain) = with.get("toolchain") else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_toolchain_missing:{id}"
        )));
    };
    for step in &job.steps {
        let StepKind::TaskExecution {
            toolchain_inputs, ..
        } = &step.kind
        else {
            continue;
        };
        let mbx_pins: Vec<&str> = toolchain_inputs
            .tools
            .iter()
            .filter_map(|selector| selector.strip_prefix("mr-boxington@"))
            .collect();
        let rust_pins: Vec<&str> = toolchain_inputs
            .tools
            .iter()
            .filter_map(|selector| selector.strip_prefix("rust@"))
            .collect();
        if mbx_pins.len() != 1
            || mbx_pins[0] != version
            || rust_pins.len() != 1
            || rust_pins[0] != rust_toolchain
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_task_toolchain_mismatch:{id}"
            )));
        }
    }
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

/// Append one guarded workspace cleanup after every job's final task step.
pub(crate) fn append_workspace_cleanups(
    jobs: &mut BTreeMap<String, Job>,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        append_workspace_cleanup(id, job)?;
    }
    Ok(())
}

fn append_workspace_cleanup(id: &str, job: &mut Job) -> Result<(), RenderError> {
    let Some((action_at, ready_at)) = lifecycle_step_indices(id, job)? else {
        return Ok(());
    };
    validate_ready_check(id, job, action_at, ready_at)?;
    if has_consumer_before_ready(job, ready_at) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_consumer_before_ready:{id}"
        )));
    }
    let cleanup = crate::cache_steps::mbx_workspace_clean_step_for_action(&job.steps[action_at])?;
    job.steps.push(cleanup);
    Ok(())
}

fn lifecycle_step_indices(id: &str, job: &Job) -> Result<Option<(usize, usize)>, RenderError> {
    let actions = role_indices(job, StepRole::MbxCache);
    let ready = role_indices(job, StepRole::MbxVersionCheck);
    let cleanups = role_indices(job, StepRole::MbxWorkspaceCleanup);
    if actions.is_empty() && ready.is_empty() && cleanups.is_empty() {
        return Ok(None);
    }
    if actions.is_empty() {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_lifecycle_without_action:{id}"
        )));
    }
    if actions.len() != 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_cleanup_action_duplicated:{id}"
        )));
    }
    if !cleanups.is_empty() {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_cleanup_duplicated:{id}"
        )));
    }
    if ready.is_empty() {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_ready_check_missing:{id}"
        )));
    }
    if ready.len() != 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_ready_check_duplicated:{id}"
        )));
    }
    Ok(Some((actions[0], ready[0])))
}

fn role_indices(job: &Job, role: StepRole) -> Vec<usize> {
    job.steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(role))
        .map(|(index, _)| index)
        .collect()
}

fn validate_ready_check(
    id: &str,
    job: &Job,
    action_at: usize,
    ready_at: usize,
) -> Result<(), RenderError> {
    if action_at >= ready_at {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_ready_check_order:{id}"
        )));
    }
    let StepKind::Action { with, env, .. } = &job.steps[action_at].kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_cleanup_action_kind_mismatch:{id}"
        )));
    };
    let version = with
        .get("version")
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_version_missing".to_owned()))?;
    let rust_toolchain = with
        .get("toolchain")
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_toolchain_missing".to_owned()))?;
    let expected = mbx_version_check_step(version, rust_toolchain, env.clone())?;
    let actual = &job.steps[ready_at];
    if actual.id != expected.id
        || actual.role != expected.role
        || actual.condition != expected.condition
        || actual.kind != expected.kind
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_ready_check_mismatch:{id}"
        )));
    }
    Ok(())
}

fn has_consumer_before_ready(job: &Job, ready_at: usize) -> bool {
    job.steps.iter().enumerate().any(|(index, step)| {
        index < ready_at && step.role != Some(StepRole::MbxVersionCheck) && uses_mbx_command(step)
    })
}

/// True for the pinned native MBX action.
pub(crate) fn is_mbx_action(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Action { uses, .. } if uses.starts_with(&format!("{MBX_ACTION_NAME}@")))
}

//! Validation that generated MBX steps match each job's selected compiler.

use std::collections::BTreeMap;

use super::mbx_command::{has_external_mbx_selector, uses_mbx_command};
use super::mbx_preflight::mbx_version_check_step;
use super::{CompileDriver, MBX_ACTION_NAME};
use velnor_actions_contract_workflow::{Job, Step, StepRole};
use velnor_actions_workflow_steps::RenderError;

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
    let velnor_actions_contract_workflow::StepKind::Action { with, env, .. } = &action.kind else {
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

/// True for the pinned native MBX action.
#[must_use]
pub fn is_mbx_action(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract_workflow::StepKind::Action { uses, .. } if uses.starts_with(&format!("{MBX_ACTION_NAME}@")))
}

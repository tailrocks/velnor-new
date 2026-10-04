//! Narrow cache-step overrides for the fixed, unmatrixed victim jobs.

use velnor_actions_contract::{Job, Step, StepKind};

use super::Phase;
use crate::{RenderError, mbx_bundle};

pub(super) fn pin_unmatrixed_key_context(job: &mut Job) -> Result<(), RenderError> {
    let key_steps: Vec<usize> = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| (step.name == mbx_bundle::MBX_BUNDLE_KEY_NAME).then_some(index))
        .collect();
    if key_steps.len() != 1 {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_key_step_identity_invalid".to_owned(),
        ));
    }
    pin_unmatrixed_key_step(&mut job.steps[key_steps[0]])
}

pub(super) fn pin_unmatrixed_key_step(step: &mut Step) -> Result<(), RenderError> {
    let env = shell_env(step)?;
    if env.get("MBX_MATRIX_CONTEXT").map(String::as_str) != Some("${{ toJSON(matrix) }}") {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_matrix_context_source_invalid".to_owned(),
        ));
    }
    if env.get("MBX_BASE_SHA").map(String::as_str)
        != Some("${{ github.event.pull_request.base.sha }}")
    {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_base_sha_source_invalid".to_owned(),
        ));
    }
    env.insert("MBX_MATRIX_CONTEXT".to_owned(), "{}".to_owned());
    env.insert("MBX_BASE_SHA".to_owned(), String::new());
    Ok(())
}

pub(super) fn pin_observer_key_step(step: &mut Step) -> Result<(), RenderError> {
    let env = shell_env(step)?;
    if env.get("MBX_MATRIX_CONTEXT").map(String::as_str) != Some("${{ toJSON(matrix) }}") {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_matrix_context_source_invalid".to_owned(),
        ));
    }
    if env.get("MBX_BASE_SHA").map(String::as_str)
        != Some("${{ steps.mbx-cancel-receipt.outputs.source_sha }}")
    {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_observer_sha_source_invalid".to_owned(),
        ));
    }
    env.insert("MBX_MATRIX_CONTEXT".to_owned(), "{}".to_owned());
    Ok(())
}

fn shell_env(
    step: &mut Step,
) -> Result<&mut std::collections::BTreeMap<String, String>, RenderError> {
    match &mut step.kind {
        StepKind::Shell { env, .. } => Ok(env),
        _ => Err(RenderError::InvalidWorkflow(
            "mbx_cancel_key_step_kind_invalid".to_owned(),
        )),
    }
}

pub(super) fn gate_victim_writer_steps(job: &mut Job, during_save: bool) {
    let writer = if during_save {
        format!(
            "success() && {} && steps.mbx-bundle.outputs.cache-hit != 'true'",
            Phase::DuringSave.gate("victim")
        )
    } else {
        "false".to_owned()
    };
    let save = format!("{writer} && steps.mbx-export.outputs.ready == 'true'");
    for step in &mut job.steps {
        match &mut step.kind {
            StepKind::Action { env, .. } | StepKind::Shell { env, .. } => {
                env.insert("MBX_GC_AUTO".to_owned(), "1".to_owned());
            }
            StepKind::Internal { .. } => {}
        }
        match step.name.as_str() {
            mbx_bundle::MBX_BUNDLE_EXPORT_NAME => step.condition = Some(writer.clone()),
            mbx_bundle::MBX_BUNDLE_SAVE_NAME => step.condition = Some(save.clone()),
            _ => {}
        }
    }
}

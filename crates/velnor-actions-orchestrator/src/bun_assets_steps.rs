use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, StepRole};
use velnor_actions_workflow_renderer::steps::internal_step;
use velnor_actions_workflow_renderer::verification_jobs::{
    bun_assets_download_step, bun_assets_upload_step,
};

use crate::OrchestratorError;
use crate::bun_assets_handoff::{
    ASSET_ID_ENV, ASSET_PINS_ENV, BUN_ASSETS_EXPORT_OP, BUN_ASSETS_IMPORT_OP, BUN_ASSETS_PREPARE_OP,
};

pub(crate) fn assets_tail(
    member_job_id: &str,
    pins: &[String],
) -> Result<Vec<Step>, OrchestratorError> {
    Ok(vec![
        handoff_op(
            "Bundle verified private assets",
            BUN_ASSETS_EXPORT_OP,
            member_job_id,
            pins,
        )?,
        bun_assets_upload_step(member_job_id)?,
    ])
}

pub(crate) fn suite_handoff(
    member_job_id: &str,
    pins: &[String],
) -> Result<Vec<Step>, OrchestratorError> {
    Ok(vec![
        handoff_op(
            "Prepare private asset handoff",
            BUN_ASSETS_PREPARE_OP,
            member_job_id,
            pins,
        )?,
        bun_assets_download_step(member_job_id)?,
        handoff_op(
            "Import and verify private assets",
            BUN_ASSETS_IMPORT_OP,
            member_job_id,
            pins,
        )?,
    ])
}

fn handoff_op(
    name: &str,
    operation: &str,
    member_job_id: &str,
    pins: &[String],
) -> Result<Step, OrchestratorError> {
    let mut step = internal_step(name, operation)?;
    step.role = Some(StepRole::BunAssetsHandoff);
    let StepKind::Internal { env, .. } = &mut step.kind else {
        return Err(OrchestratorError::Contract {
            problem: "bun_assets_internal_step_kind".to_owned(),
        });
    };
    env.extend(BTreeMap::from([
        (ASSET_ID_ENV.to_owned(), member_job_id.to_owned()),
        (ASSET_PINS_ENV.to_owned(), pins.join(" ")),
    ]));
    Ok(step)
}

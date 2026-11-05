// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{COMPLETE_STATE_TRANSPORT_EXPR, SliceId};
use velnor_actions_contract::workflow::state_seal::{
    STATE_PRODUCED_CONDITION, STATE_PRODUCER_ARTIFACT_OUTPUT, STATE_PRODUCER_PRODUCED_OUTPUT,
    STATE_SEAL_ARTIFACT_RETENTION_DAYS, STATE_UPLOAD_CONDITION,
};
use velnor_actions_contract::{Step, StepId, StepRole};

use crate::RenderError;
use crate::steps::{UPLOAD_ARTIFACT_USES, action_step, shell_step};

pub fn state_payload_upload_step(slice: &SliceId) -> Result<Step, RenderError> {
    let mut step = action_step(
        "Upload state payload",
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "name".to_owned(),
                format!(
                    "velnor-state-{slice}-r${{{{ github.run_id }}}}-a${{{{ github.run_attempt }}}}"
                ),
            ),
            ("path".to_owned(), COMPLETE_STATE_TRANSPORT_EXPR.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            (
                "retention-days".to_owned(),
                STATE_SEAL_ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )?;
    step.id = Some(StepId::StateSealUpload);
    step.role = Some(StepRole::StateSealUpload);
    step.condition = Some(STATE_UPLOAD_CONDITION.to_owned());
    Ok(step)
}

pub fn state_produced_step() -> Result<Step, RenderError> {
    let mut step = shell_step(
        "Record state produced",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!("printf '%s\\n' '{STATE_PRODUCER_PRODUCED_OUTPUT}=true' >> \"$GITHUB_OUTPUT\""),
        ],
        BTreeMap::new(),
    )?;
    step.id = Some(StepId::StateProduced);
    step.role = Some(StepRole::StateProduced);
    step.condition = Some(STATE_PRODUCED_CONDITION.to_owned());
    Ok(step)
}

#[must_use]
pub fn state_producer_outputs() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            STATE_PRODUCER_ARTIFACT_OUTPUT.to_owned(),
            format!(
                "${{{{ steps.{}.outputs.artifact-id }}}}",
                StepId::StateSealUpload.as_str()
            ),
        ),
        (
            STATE_PRODUCER_PRODUCED_OUTPUT.to_owned(),
            format!(
                "${{{{ steps.{}.outputs.{STATE_PRODUCER_PRODUCED_OUTPUT} }}}}",
                StepId::StateProduced.as_str()
            ),
        ),
    ])
}

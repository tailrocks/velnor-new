// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{COMPLETE_STATE_TRANSPORT_EXPR, SliceId};
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_ARTIFACT_RETENTION_DAYS, STATE_UPLOAD_CONDITION, state_payload_artifact_name,
    state_producer_artifact_output, state_producer_artifact_value,
};
use velnor_actions_contract::{Step, StepId, StepRole};

use crate::RenderError;
use crate::steps::{UPLOAD_ARTIFACT_USES, action_step};

pub fn state_payload_upload_step() -> Result<Step, RenderError> {
    let mut step = action_step(
        "Upload state payload",
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), state_payload_artifact_name()),
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

#[must_use]
pub fn state_producer_outputs<'a>(
    slices: impl IntoIterator<Item = &'a SliceId>,
) -> BTreeMap<String, String> {
    slices
        .into_iter()
        .map(|slice| {
            (
                state_producer_artifact_output(slice),
                state_producer_artifact_value(slice),
            )
        })
        .collect()
}

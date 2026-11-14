// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepRole};

use crate::RenderError;
use crate::steps::shell_step;

const ARTIFACT_ID_GUARD: &str = "set -eu; case \"$VELNOR_EVIDENCE_ARTIFACT_ID\" in ''|*[!0-9]*|0*) exit 1;; esac; [ \"${#VELNOR_EVIDENCE_ARTIFACT_ID}\" -le 16 ] || exit 1; if [ \"${#VELNOR_EVIDENCE_ARTIFACT_ID}\" -eq 16 ]; then [ \"$VELNOR_EVIDENCE_ARTIFACT_ID\" -le 9007199254740991 ] || exit 1; fi";

pub(super) fn step(artifact: &str) -> Result<Step, RenderError> {
    let mut guard = shell_step(
        "Validate exact artifact ID",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            ARTIFACT_ID_GUARD.to_owned(),
        ],
        BTreeMap::from([(
            "VELNOR_EVIDENCE_ARTIFACT_ID".to_owned(),
            artifact.to_owned(),
        )]),
    )?;
    guard.role = Some(StepRole::StateSealArtifactGuard);
    Ok(guard)
}

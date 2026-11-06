// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::steps;
use velnor_actions_contract::{Step, StepKind, StepRole};

pub(super) fn required_verdict_step(step: &Step) -> bool {
    match &step.kind {
        StepKind::Action { uses, with, .. } => match step.role {
            Some(StepRole::Checkout) => uses.starts_with("actions/checkout@"),
            Some(StepRole::MiseSetup) => {
                uses.starts_with("jdx/mise-action@")
                    && ["install", "env", "cache", "cache_save"]
                        .iter()
                        .all(|key| with.get(*key).is_some_and(|value| value == "false"))
            }
            Some(StepRole::AcquireProtectedPreview) => {
                crate::steps::protected_preview_acquire_step()
                    .is_ok_and(|approved| approved == *step)
            }
            Some(StepRole::DownloadPlan | StepRole::AttestationDownload) => {
                uses == crate::steps::DOWNLOAD_ARTIFACT_USES
            }
            Some(StepRole::PublishFinal) => uses == crate::steps::UPLOAD_ARTIFACT_USES,
            _ => false,
        },
        StepKind::Shell { .. } => {
            matches!(
                step.role,
                Some(StepRole::AcquireVelnor | StepRole::PreparePinnedTools)
            )
        }
        StepKind::Internal { operation, .. } => {
            operation == steps::MERGE_OPERATION
                || operation == steps::FETCH_OPERATION
                || step == &crate::steps_internal::group_ledger_steps::assemble_group_ledgers_step()
                || operation
                    == &format!(
                        "{}:{}",
                        steps::WRITE_REQUEST_OPERATION,
                        steps::MERGE_OPERATION
                    )
        }
        StepKind::TaskExecution { .. } | StepKind::NativeCheckExecution { .. } => false,
    }
}

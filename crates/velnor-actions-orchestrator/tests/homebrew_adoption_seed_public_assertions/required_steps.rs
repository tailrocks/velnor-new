// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, io};
use velnor_actions_contract::{Step, StepKind, StepRole};

pub(crate) fn assert_trusted_required_steps(
    steps: &[Step],
    acquire: usize,
) -> Result<(), Box<dyn Error>> {
    for (index, step) in steps.iter().enumerate() {
        match &step.kind {
            StepKind::Action { uses, .. } => assert!(
                [
                    "actions/checkout@",
                    "jdx/mise-action@",
                    "actions/download-artifact@",
                    "actions/upload-artifact@"
                ]
                .iter()
                .any(|prefix| uses.starts_with(prefix)),
                "candidate or cache action in Required: {uses}"
            ),
            StepKind::Shell { env, .. } => assert!(
                matches!(
                    step.role,
                    Some(StepRole::AcquireVelnor | StepRole::PreparePinnedTools)
                ) || is_assemble_group_ledgers_step(step, env),
                "candidate shell in Required: {}",
                step.name
            ),
            StepKind::Internal { .. } => {
                assert!(index > acquire, "native operation preceded acquisition");
            }
            other => {
                return Err(io::Error::other(format!(
                    "candidate execution in Required: {other:?}"
                ))
                .into());
            }
        }
    }
    Ok(())
}

fn is_assemble_group_ledgers_step(
    step: &Step,
    env: &std::collections::BTreeMap<String, String>,
) -> bool {
    step.name == "Assemble group ledgers"
        && step.role.is_none()
        && step.condition.is_none()
        && env
            .get(velnor_actions_workflow_renderer::steps::INTERNAL_OP_ENV)
            .map(String::as_str)
            == Some("assemble-group-ledgers-v1")
}

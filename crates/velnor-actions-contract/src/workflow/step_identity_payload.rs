// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::step_payloads::{valid_configured_checkout_payload, valid_tool_seed_payload};
use super::{JACTIONLINT_VERIFIER_USES, RUST_VERIFIER_USES, Step, StepKind, StepRole};

pub(super) fn valid_rust_verifier_payload(kind: &StepKind) -> bool {
    match kind {
        StepKind::Shell { .. } => true,
        StepKind::Action { uses, with, env } => {
            RUST_VERIFIER_USES.contains(&uses.as_str()) && with.is_empty() && env.is_empty()
        }
        StepKind::TaskExecution { .. }
        | StepKind::NativeCheckExecution { .. }
        | StepKind::Internal { .. } => false,
    }
}

pub(super) fn valid_jactionlint_verifier_payload(kind: &StepKind) -> bool {
    match kind {
        StepKind::Shell { .. } => true,
        StepKind::Action { uses, with, env } => {
            uses == JACTIONLINT_VERIFIER_USES && with.is_empty() && env.is_empty()
        }
        StepKind::TaskExecution { .. }
        | StepKind::NativeCheckExecution { .. }
        | StepKind::Internal { .. } => false,
    }
}

#[must_use]
pub fn is_configured_checkout(step: &Step, expected_uses: &str) -> bool {
    step.role == Some(StepRole::Checkout)
        && step.condition.is_none()
        && matches!(&step.kind, StepKind::Action { uses, .. } if uses == expected_uses)
        && valid_configured_checkout_payload(&step.kind)
}

#[must_use]
pub fn is_tool_seed_step(step: &Step) -> bool {
    step.role == Some(StepRole::ToolSeed)
        && step.condition.is_none()
        && valid_tool_seed_payload(&step.kind)
}

pub(super) fn action_has_prefix_for_kind(kind: &StepKind, prefix: &str) -> bool {
    matches!(kind, StepKind::Action { uses, .. } if action_has_prefix(uses, prefix))
}

fn action_has_prefix(uses: &str, prefix: &str) -> bool {
    uses.starts_with(prefix)
}

pub(super) fn internal_operation(kind: &StepKind, expected: &str) -> bool {
    matches!(kind, StepKind::Internal { operation, .. } if operation == expected)
}

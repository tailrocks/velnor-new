//! Payload predicates used by semantic step identity validation.

use super::super::step_payloads::{valid_configured_checkout_payload, valid_tool_seed_payload};
use super::{RUST_VERIFIER_USES, Step, StepKind, StepRole};

/// Preserve the original shell role and admit only the two empty-input
/// generated action calls used for the fixed restored/installed phases.
pub(super) fn valid_rust_verifier_payload(kind: &StepKind) -> bool {
    match kind {
        StepKind::Shell { .. } => true,
        StepKind::Action { uses, with, env } => {
            RUST_VERIFIER_USES.contains(&uses.as_str()) && with.is_empty() && env.is_empty()
        }
        StepKind::TaskExecution { .. } | StepKind::Internal { .. } => false,
    }
}

/// Check that a step is the configured checkout owner in this workflow scope.
///
/// Checkout is a semantic role, not a display-name convention. The caller
/// supplies the exact pinned action ref from its validated render context.
#[must_use]
pub fn is_configured_checkout(step: &Step, expected_uses: &str) -> bool {
    step.role == Some(StepRole::Checkout)
        && step.condition.is_none()
        && matches!(&step.kind, StepKind::Action { uses, .. } if uses == expected_uses)
        && valid_configured_checkout_payload(&step.kind)
}

/// Check that a step is the unconditional local tool-seed consumer.
#[must_use]
pub fn is_tool_seed_step(step: &Step) -> bool {
    step.role == Some(StepRole::ToolSeed)
        && step.condition.is_none()
        && valid_tool_seed_payload(&step.kind)
}

/// True when an action payload uses a fixed name prefix.
pub(super) fn action_has_prefix_for_kind(kind: &StepKind, prefix: &str) -> bool {
    matches!(kind, StepKind::Action { uses, .. } if action_has_prefix(uses, prefix))
}

/// True when a pinned action ref starts with its required owner/repository.
fn action_has_prefix(uses: &str, prefix: &str) -> bool {
    uses.starts_with(prefix)
}

/// True when an internal operation matches its role contract.
pub(super) fn internal_operation(kind: &StepKind, expected: &str) -> bool {
    matches!(kind, StepKind::Internal { operation, .. } if operation == expected)
}

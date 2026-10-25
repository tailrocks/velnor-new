use super::super::step::{Step, StepKind};
use super::super::step_payloads::{valid_configured_checkout_payload, valid_tool_seed_payload};
use super::{
    CACHE_TAR_LAUNCHER_SHA256_INPUT, CACHE_TAR_LAUNCHER_USES, JACTIONLINT_VERIFIER_USES,
    PRESEED_HELPER_SHA256_OUTPUT_EXPR, RUST_VERIFIER_USES, StepRole,
    TOOLS_CACHE_IDENTITY_DIGEST_INPUT, TOOLS_CACHE_PRELUDE_USES,
};

/// Preserve inline Rust verification and admit only the fixed empty-binding actions.
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

/// Preserve inline jactionlint verification and admit only its fixed empty-binding action.
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

/// Require the fixed generated hosted identity-and-seed composite and digest.
pub(in crate::workflow) fn valid_tools_cache_identity(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, env }
        if TOOLS_CACHE_PRELUDE_USES.contains(&uses.as_str())
            && with.len() == 1
            && with.get(TOOLS_CACHE_IDENTITY_DIGEST_INPUT).is_some_and(|digest| {
                crate::config::project_tool_refs::is_tools_identity_input(digest)
            })
            && env.is_empty())
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

pub(in crate::workflow) fn valid_cache_tar_launcher(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, env }
    if uses == CACHE_TAR_LAUNCHER_USES
        && env.is_empty()
        && with.len() == 1
        && with.get(CACHE_TAR_LAUNCHER_SHA256_INPUT).is_some_and(|digest| {
            crate::ids::is_lower_hex_len(digest, 64)
                || digest == PRESEED_HELPER_SHA256_OUTPUT_EXPR
        }))
}

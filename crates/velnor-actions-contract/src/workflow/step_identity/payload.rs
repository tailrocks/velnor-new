use super::super::step::StepKind;
use super::{
    CACHE_TAR_LAUNCHER_SHA256_INPUT, CACHE_TAR_LAUNCHER_USES, PRESEED_HELPER_SHA256_OUTPUT_EXPR,
    TOOLS_CACHE_IDENTITY_DIGEST_INPUT, TOOLS_CACHE_PRELUDE_USES,
};

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

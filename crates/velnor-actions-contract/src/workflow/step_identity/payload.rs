use super::super::step::StepKind;
use super::{
    CACHE_TAR_LAUNCHER_SHA256_INPUT, CACHE_TAR_LAUNCHER_USES, PRESEED_HELPER_SHA256_OUTPUT_EXPR,
    TOOL_SEED_USES, TOOLS_CACHE_IDENTITY_DIGEST_INPUT, TOOLS_CACHE_PRELUDE_USES,
};

/// Require the standard credential-free checkout action payload.
pub(super) fn valid_checkout_payload(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
        if action_has_prefix(uses, "actions/checkout@")
            && with.get("persist-credentials").is_some_and(|value| value == "false"))
}

/// Require the simple generated checkout shape used before host-seed reads.
pub(super) fn valid_configured_checkout_payload(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { with, env, .. }
        if valid_checkout_payload(kind)
            && (with.len() == 1
                || (with.len() == 2
                    && with.get("fetch-depth").is_some_and(|value| value == "0")))
            && env.is_empty())
}

/// Require the fixed local seed action and its one explicit cache identity.
pub(super) fn valid_tool_seed_payload(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, env }
        if uses == TOOL_SEED_USES
            && with.len() == 1
            && with.get("cache_key").is_some_and(|key| !key.is_empty())
            && env.is_empty())
}

/// Require the fixed generated hosted identity-and-seed composite and digest.
pub(in crate::workflow) fn valid_tools_cache_identity(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, env }
        if TOOLS_CACHE_PRELUDE_USES.contains(&uses.as_str())
            && with.len() == 1
            && with.get(TOOLS_CACHE_IDENTITY_DIGEST_INPUT).is_some_and(|digest| {
                crate::config::project_tool_refs::is_tools_cache_identity_input(digest)
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

/// Validate the typed Mise setup payload shape shared by render paths.
pub(super) fn valid_mise_setup(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
        if action_has_prefix(uses, "jdx/mise-action@")
            && with.get("version").is_some_and(|value| !value.is_empty())
            && with.get("sha256").is_some_and(|value| !value.is_empty())
            && with.get("install").is_some_and(|value| value == "false")
            && with.get("env").is_some_and(|value| value == "false")
            && with.get("cache").is_some_and(|value| value == "false" || value == "true")
            && with.get("cache_save").is_some_and(|value| value == "false" || value == "true"))
}

/// Validate the local provider admission call and exact same-restore outputs.
/// Validate the native MBX action that owns the object-cache lifecycle.
pub(super) fn valid_mbx_cache(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
        if action_has_prefix(uses, "jdx/mr-boxington-action@")
            && with.get("github-cache-mode").is_some_and(|mode| mode == "target" || mode == "objects")
            && with.get("version").is_some_and(|version| !version.is_empty())
            && with.get("toolchain").is_some_and(|toolchain| !toolchain.is_empty())
            && with.get("cache-generation").is_some_and(|generation| !generation.is_empty()))
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

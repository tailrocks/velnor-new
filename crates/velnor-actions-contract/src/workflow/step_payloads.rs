//! Fixed payload shapes of the Mise setup and tool-seed roles.

use super::step::StepKind;
use super::step_identity::TOOL_SEED_USES;

/// Validate the typed Mise setup payload shape shared by render paths.
pub(super) fn valid_mise_setup(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
        if uses.starts_with("jdx/mise-action@")
            && with.get("version").is_some_and(|value| !value.is_empty())
            && with.get("sha256").is_some_and(|value| !value.is_empty())
            && with.get("install").is_some_and(|value| value == "false")
            && with.get("env").is_some_and(|value| value == "false")
            && with.get("cache").is_some_and(|value| value == "false" || value == "true")
            && with.get("cache_save").is_some_and(|value| value == "false" || value == "true"))
}

/// Require the fixed local seed action and its one explicit cache identity.
pub(super) fn valid_tool_seed_payload(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, env }
        if uses == TOOL_SEED_USES
            && with.len() == 1
            && with.get("cache_key").is_some_and(|key| !key.is_empty())
            && env.is_empty())
}

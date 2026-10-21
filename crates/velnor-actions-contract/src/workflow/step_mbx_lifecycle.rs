//! Cross-step validation for the native MBX cleanup lifecycle.

use super::{Step, StepKind, StepRole};
use crate::errors::ContractError;

/// Require a native action, its ready guard, and cleanup in one ordered job scope.
pub(super) fn validate_mbx_cleanup_sequence(
    steps: &[Step],
    scope: &str,
) -> Result<(), ContractError> {
    let actions = positions(steps, StepRole::MbxCache);
    let ready = positions(steps, StepRole::MbxVersionCheck);
    let cleanups = positions(steps, StepRole::MbxWorkspaceCleanup);
    if cleanups.is_empty() {
        return Ok(());
    }
    if actions.len() != 1
        || ready.len() != 1
        || cleanups.len() != 1
        || actions[0] >= ready[0]
        || ready[0] >= cleanups[0]
        || cleanups[0] + 1 != steps.len()
    {
        return Err(ContractError::identity(
            "job.steps",
            format!("mbx_cleanup_sequence_mismatch:{scope}"),
        ));
    }
    Ok(())
}

fn positions(steps: &[Step], role: StepRole) -> Vec<usize> {
    steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(role))
        .map(|(index, _)| index)
        .collect()
}

/// Validate the native MBX action: install-only (`backend: local`) or the qualification objects mode.
pub(super) fn valid_mbx_cache(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
    if uses.starts_with("jdx/mr-boxington-action@")
        && with.get("version").is_some_and(|version| !version.is_empty())
        && with.get("toolchain").is_some_and(|toolchain| !toolchain.is_empty())
        && match with.get("backend").map(String::as_str) {
            // Install-only native action; Velnor's own steps move the bundle.
            Some("local") => !with.contains_key("github-cache-mode"),
            // Qualification keeps the action-owned objects transport.
            None => with.get("github-cache-mode").is_some_and(|mode| mode == "objects")
                && with.get("cache-generation").is_some_and(|generation| !generation.is_empty()),
            Some(_) => false,
        })
}

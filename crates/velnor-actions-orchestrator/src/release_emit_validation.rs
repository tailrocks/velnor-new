// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{OrchestratorError, ReleaseRegistry, derive_release_inputs, enabled_release};

/// Validate the configured crates.io release against the approved source workspace.
pub fn validate_crate_release_workspace(
    root: &std::path::Path,
    guard: Option<&crate::OperatorToolchainGuard>,
) -> Result<(), OrchestratorError> {
    let prep = match guard {
        Some(guard) => {
            crate::planner::prepare::prepare_for_candidate_with_operator_guard(root, guard)?
        }
        None => crate::planner::prepare::prepare_for_candidate(root)?,
    };
    let release = enabled_release(&prep)?.ok_or_else(|| {
        OrchestratorError::unsupported("crate_release_validation", "release is not enabled")
    })?;
    if release.registry != ReleaseRegistry::CratesIo {
        return Err(OrchestratorError::unsupported(
            "crate_release_validation",
            "registry is not crates.io",
        ));
    }
    let inputs = derive_release_inputs(&prep, release)?;
    inputs.intent.validate()?;
    Ok(())
}

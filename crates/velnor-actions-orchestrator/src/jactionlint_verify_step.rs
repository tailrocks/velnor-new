//! `Verify jactionlint`: Velnor's own check of the installed linter bytes.
//!
//! Mise's `github:jdx/jactionlint` backend already checks the release-API
//! digest and the GitHub artifact attestation, which trust GitHub and the
//! upstream release workflow. This step adds the Velnor-pinned control,
//! modelled on `Verify Rust channel manifest`: it resolves the installed
//! `jactionlint` executable through `mise which` and compares its SHA-256
//! with [`velnor_actions_mise::JACTIONLINT_DIGESTS`] for the runner platform.
//! The catalog pin must have a digest; a project-declared version without
//! one (hard decision 4) keeps Mise's attestation check and gets a notice.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;

/// Contract-fixed display name of the executable verification step.
pub(crate) const VERIFY_JACTIONLINT_STEP: &str = "Verify jactionlint";

/// Stable failure code for a catalog pin without an executable digest.
#[cfg(test)]
pub(crate) const NOT_CATALOGUED_CODE: &str = "jactionlint_executable_not_catalogued";

/// Fixed POSIX script with one byte-exact multiline Setup Mise binder.
///
/// The renderer owns the canonical producer and validator so a role-tagged
/// script cannot move the binder into a shell control scope.
pub(crate) fn script(catalog: &ToolCatalog) -> String {
    velnor_actions_workflow_renderer::jactionlint_verifier_script(
        catalog.version(velnor_actions_mise::PinnedTool::Jactionlint),
    )
}

/// `sh -c` argv for the verification script (release-gate steps).
pub(crate) fn argv(catalog: &ToolCatalog) -> Vec<String> {
    vec!["sh".to_owned(), "-c".to_owned(), script(catalog)]
}

/// Typed, credential-scrubbed verification step after `Prepare pinned tools`.
///
/// # Errors
///
/// Returns a contract error when the renderer rejects the fixed argv.
pub(crate) fn verify_jactionlint_step(catalog: &ToolCatalog) -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::steps::jactionlint_verifier_step(
        VERIFY_JACTIONLINT_STEP,
        argv(catalog),
        BTreeMap::new(),
    )
    .map_err(OrchestratorError::from)
}

#[cfg(test)]
#[path = "jactionlint_verify_step_tests.rs"]
mod tests;

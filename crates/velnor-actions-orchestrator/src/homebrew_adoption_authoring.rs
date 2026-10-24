//! Data-only authoring for the protected one-time Homebrew seed and cutover.

use std::path::Path;

use serde::Serialize;
use velnor_actions_mise::CheckDeadline;

use crate::OrchestratorError;

#[path = "homebrew_adoption_authoring_cutover.rs"]
mod cutover;
#[path = "homebrew_adoption_authoring_output.rs"]
mod output;
#[path = "homebrew_adoption_authoring_seed.rs"]
mod seed;
#[cfg(test)]
#[path = "homebrew_adoption_authoring_tests.rs"]
mod tests;

const AUTHORING_BUDGET_SECONDS: u64 = 5 * 60;

/// Reviewable result from one exact Homebrew adoption authoring operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HomebrewAdoptionAuthoringReport {
    /// Fresh output directory containing data-only overlay files.
    pub output_dir: String,
    /// Exact authenticated protected base commit.
    pub base_sha: String,
    /// Overlay files written beneath `output_dir`.
    pub written_paths: Vec<String>,
    /// Exact source files the overlay proposes deleting.
    pub deleted_paths: Vec<String>,
}

/// Author the exact protected initial seed overlay from a published SDK release.
///
/// # Errors
/// Returns an error when the manifest is unauthenticated, the protected tap
/// base differs from the compiled transition, or an exclusive output cannot be
/// published safely.
pub fn author_homebrew_adoption_seed(
    transition_id: &str,
    sdk_manifest: &Path,
    output_dir: &Path,
) -> Result<HomebrewAdoptionAuthoringReport, OrchestratorError> {
    let deadline = authoring_deadline()?;
    seed::author(transition_id, sdk_manifest, output_dir, deadline)
}

/// Author the normal structural cutover over the authenticated seed output base.
///
/// # Errors
/// Returns an error when the source authority, seed witness, published SDK, or
/// current tap base cannot be authenticated, or output publication fails.
pub fn author_homebrew_adoption_cutover(
    transition_id: &str,
    sdk_manifest: &Path,
    output_dir: &Path,
) -> Result<HomebrewAdoptionAuthoringReport, OrchestratorError> {
    let deadline = authoring_deadline()?;
    cutover::author(transition_id, sdk_manifest, output_dir, deadline)
}

fn authoring_deadline() -> Result<CheckDeadline, OrchestratorError> {
    CheckDeadline::after(std::time::Duration::from_secs(AUTHORING_BUDGET_SECONDS))
        .map_err(|_| crate::internal::internal("homebrew_adoption_authoring_deadline"))
}
